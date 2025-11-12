// This is the leveling module - it contains ALL the business logic for the leveling system.
// Notice how this module has NO Discord-specific code (no serenity, no poise imports).
// It works with primitive types (u64, String) so it could theoretically be used
// in a web app, CLI tool, or any other frontend.

use async_trait::async_trait;
use std::time::{Duration, Instant};
use thiserror::Error;

// ============================================================================
// DOMAIN MODELS
// ============================================================================
// These structs represent the core concepts in our leveling domain.

#[allow(dead_code)]
/// Represents a user's XP and level data for a specific guild.
///
/// **Why separate user_id and guild_id?**
/// Users can be in multiple Discord servers (guilds), and we want to track
/// their progress separately in each one.
#[derive(Debug, Clone)]
pub struct UserStats {
    pub user_id: u64,
    pub guild_id: u64,
    pub xp: u64,
    pub level: u32,
    /// When did this user last gain XP? Used for cooldown prevention.
    pub last_xp_gain: Option<Instant>,
}

#[allow(dead_code)]
/// Represents when a user levels up.
/// This is returned by the service so the Discord layer can announce it.
#[derive(Debug, Clone)]
pub struct LevelUpEvent {
    pub user_id: u64,
    pub guild_id: u64,
    pub old_level: u32,
    pub new_level: u32,
    pub total_xp: u64,
}

/// Tracks where XP came from (for future analytics or different XP rates).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum XpSource {
    Message,
    VoiceMinute,
    CodeChallenge {
        difficulty: Difficulty,
        language: String,
        execution_time_ms: u64,
    },
}

/// Difficulty levels for code challenges (used in XP calculation).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

#[allow(dead_code)]
impl Difficulty {
    /// How much XP should each difficulty award?
    /// This is business logic - it belongs in the core, not in Discord commands.
    pub fn xp_reward(&self) -> u64 {
        match self {
            Difficulty::Easy => 50,
            Difficulty::Medium => 150,
            Difficulty::Hard => 500,
            Difficulty::Expert => 1000,
        }
    }
}

// ============================================================================
// ERRORS
// ============================================================================
// We define our own error types rather than using generic errors.
// This makes error handling explicit and documents what can go wrong.

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum LevelingError {
    #[error("User is on cooldown. Time remaining: {0:?}")]
    OnCooldown(Duration),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Invalid user or guild ID")]
    InvalidId,
}

// ============================================================================
// STORAGE TRAIT (PORT)
// ============================================================================
// This is the "port" in hexagonal architecture.
// The core defines WHAT it needs, but not HOW it's implemented.
// The infra layer will provide the actual implementation (database, in-memory, etc.).

/// Trait for persisting XP data.
///
/// **Why a trait?**
/// - Allows different implementations (in-memory for testing, database for production)
/// - Makes the core testable without needing a real database
/// - Follows Dependency Inversion Principle (core depends on abstraction, not concrete implementation)
#[async_trait]
pub trait XpStore: Send + Sync {
    /// Get a user's current XP in a guild.
    /// Returns 0 if the user has never gained XP in this guild.
    async fn get_xp(&self, user_id: u64, guild_id: u64) -> Result<u64, LevelingError>;

    /// Add XP to a user's total in a guild.
    /// This should be atomic (no race conditions if called multiple times).
    async fn add_xp(&self, user_id: u64, guild_id: u64, amount: u64) -> Result<(), LevelingError>;

    /// Get the top users in a guild by XP.
    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<UserStats>, LevelingError>;

    /// Update the last XP gain time for cooldown tracking.
    async fn update_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
        time: Instant,
    ) -> Result<(), LevelingError>;

    /// Get the last time a user gained XP (for cooldown).
    async fn get_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<Instant>, LevelingError>;
}

// ============================================================================
// CORE SERVICE
// ============================================================================
// This is where the business logic lives.
// The service orchestrates operations using the storage trait.

#[allow(dead_code)]
/// The main service for leveling operations.
///
/// **Generic over S: XpStore**
/// This means we can use ANY implementation of XpStore.
/// The service doesn't care if it's a database, file, or in-memory - it just uses the trait.
pub struct LevelingService<S: XpStore> {
    /// The storage implementation (injected via constructor).
    store: S,

    /// How much XP to award per message.
    xp_per_message: u64,

    /// Cooldown between XP gains (prevents spam).
    cooldown: Duration,
}

impl<S: XpStore> LevelingService<S> {
    /// Create a new leveling service with the given storage implementation.
    ///
    /// **Dependency Injection:**
    /// We pass in the storage implementation rather than creating it here.
    /// This is a key principle of Clean Architecture.
    pub fn new(store: S) -> Self {
        Self {
            store,
            xp_per_message: 15,                // Default: 15 XP per message
            cooldown: Duration::from_secs(60), // Default: 1 minute cooldown
        }
    }

    fn validate_ids(user_id: u64, guild_id: u64) -> Result<(), LevelingError> {
        if user_id == 0 || guild_id == 0 {
            Err(LevelingError::InvalidId)
        } else {
            Ok(())
        }
    }

    fn validate_guild_id(guild_id: u64) -> Result<(), LevelingError> {
        if guild_id == 0 {
            Err(LevelingError::InvalidId)
        } else {
            Ok(())
        }
    }

    /// Process a message and potentially award XP.
    ///
    /// **Returns:**
    /// - `Ok(Some(LevelUpEvent))` if the user leveled up
    /// - `Ok(None)` if XP was awarded but no level up occurred
    /// - `Err(LevelingError::OnCooldown)` if the user is on cooldown
    /// - `Err(...)` for storage errors
    pub async fn process_message(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<LevelUpEvent>, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;

        // 1. Check cooldown
        if let Some(last_time) = self.store.get_last_xp_time(user_id, guild_id).await? {
            let elapsed = Instant::now().duration_since(last_time);
            if elapsed < self.cooldown {
                let remaining = self.cooldown - elapsed;
                return Err(LevelingError::OnCooldown(remaining));
            }
        }

        // 2. Get current XP and calculate current level
        let current_xp = self.store.get_xp(user_id, guild_id).await?;
        let old_level = self.calculate_level(current_xp);

        // 3. Award XP
        self.store
            .add_xp(user_id, guild_id, self.xp_per_message)
            .await?;
        let new_xp = current_xp + self.xp_per_message;

        // 4. Update cooldown timestamp
        self.store
            .update_last_xp_time(user_id, guild_id, Instant::now())
            .await?;

        // 5. Check if user leveled up
        let new_level = self.calculate_level(new_xp);

        if new_level > old_level {
            Ok(Some(LevelUpEvent {
                user_id,
                guild_id,
                old_level,
                new_level,
                total_xp: new_xp,
            }))
        } else {
            Ok(None)
        }
    }

    /// Calculate level from total XP.
    ///
    /// **Formula:** Uses a square root progression so levels get progressively harder.
    /// Level 1 = 100 XP, Level 2 = 255 XP, Level 3 = 464 XP, etc.
    ///
    /// This is PURE business logic - no side effects, just math.
    pub fn calculate_level(&self, xp: u64) -> u32 {
        // Formula: level = floor(sqrt(xp / 50))
        ((xp as f64 / 50.0).sqrt().floor() as u32).max(1)
    }

    /// Calculate how much XP is needed for the next level.
    pub fn xp_for_next_level(&self, current_level: u32) -> u64 {
        // Inverse of calculate_level formula
        let next_level = current_level + 1;
        (next_level as u64).pow(2) * 50
    }

    /// Get a user's current stats.
    pub async fn get_user_stats(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<UserStats, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;

        let xp = self.store.get_xp(user_id, guild_id).await?;
        let level = self.calculate_level(xp);
        let last_xp_gain = self.store.get_last_xp_time(user_id, guild_id).await?;

        Ok(UserStats {
            user_id,
            guild_id,
            xp,
            level,
            last_xp_gain,
        })
    }

    /// Get the leaderboard for a guild.
    pub async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<UserStats>, LevelingError> {
        Self::validate_guild_id(guild_id)?;

        self.store.get_leaderboard(guild_id, limit).await
    }

    /// Award XP from a source other than messages (like code challenges).
    pub async fn award_xp(
        &self,
        user_id: u64,
        guild_id: u64,
        amount: u64,
        source: XpSource,
    ) -> Result<Option<LevelUpEvent>, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;

        let bonus_xp = match &source {
            XpSource::Message => 0,
            XpSource::VoiceMinute => 0,
            XpSource::CodeChallenge {
                difficulty,
                language,
                execution_time_ms,
            } => {
                let mut bonus = difficulty.xp_reward();
                if language.eq_ignore_ascii_case("rust") {
                    bonus += 25;
                }
                if *execution_time_ms <= 1_000 {
                    bonus += 10;
                }
                bonus
            }
        };
        let total_amount = amount.saturating_add(bonus_xp);

        let current_xp = self.store.get_xp(user_id, guild_id).await?;
        let old_level = self.calculate_level(current_xp);

        self.store.add_xp(user_id, guild_id, total_amount).await?;
        let new_xp = current_xp + total_amount;
        let new_level = self.calculate_level(new_xp);

        if new_level > old_level {
            Ok(Some(LevelUpEvent {
                user_id,
                guild_id,
                old_level,
                new_level,
                total_xp: new_xp,
            }))
        } else {
            Ok(None)
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================
// Core logic should be thoroughly tested since it contains your business rules.

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct NoopStore;

    #[async_trait]
    impl XpStore for NoopStore {
        async fn get_xp(&self, _: u64, _: u64) -> Result<u64, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn add_xp(&self, _: u64, _: u64, _: u64) -> Result<(), LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn get_leaderboard(&self, _: u64, _: usize) -> Result<Vec<UserStats>, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn update_last_xp_time(
            &self,
            _: u64,
            _: u64,
            _: Instant,
        ) -> Result<(), LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn get_last_xp_time(&self, _: u64, _: u64) -> Result<Option<Instant>, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }
    }

    fn make_service() -> LevelingService<NoopStore> {
        LevelingService::new(NoopStore)
    }

    #[test]
    fn test_level_calculation() {
        let service = make_service();

        assert_eq!(service.calculate_level(0), 1);
        assert_eq!(service.calculate_level(50), 1);
        assert_eq!(service.calculate_level(100), 1);
        assert_eq!(service.calculate_level(200), 2);
        assert_eq!(service.calculate_level(450), 3);
    }

    #[test]
    fn test_xp_for_next_level() {
        let service = make_service();

        assert_eq!(service.xp_for_next_level(1), 200);
        assert_eq!(service.xp_for_next_level(2), 450);
        assert_eq!(service.xp_for_next_level(3), 800);
    }

    #[test]
    fn difficulty_rewards_are_progressive() {
        assert!(Difficulty::Easy.xp_reward() < Difficulty::Medium.xp_reward());
        assert!(Difficulty::Medium.xp_reward() < Difficulty::Hard.xp_reward());
        assert!(Difficulty::Hard.xp_reward() < Difficulty::Expert.xp_reward());
    }

    #[test]
    fn leveling_error_messages_are_descriptive() {
        let storage_error = LevelingError::StorageError("db down".into());
        assert!(storage_error.to_string().contains("db down"));

        let invalid_id = LevelingError::InvalidId;
        assert_eq!(invalid_id.to_string(), "Invalid user or guild ID");
    }
}
