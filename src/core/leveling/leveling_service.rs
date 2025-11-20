// This is the leveling module - it contains ALL the business logic for the leveling system.
#![allow(dead_code)]
// Notice how this module has NO Discord-specific code (no serenity, no poise imports).
// It works with primitive types (u64, String) so it could theoretically be used
// in a web app, CLI tool, or any other frontend.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use thiserror::Error;

#[path = "achievements.rs"]
pub mod achievements;
use achievements::{get_all_achievements, Achievement};

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

// Rich user profile that mirrors the python 'user_data' dictionary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: u64,
    pub guild_id: u64,
    pub level: u32,
    pub total_xp: u64,
    pub xp_to_next_level: u64,
    pub total_commands_used: u64,
    pub total_messages: u64,
    pub last_daily: Option<DateTime<Utc>>,
    pub daily_streak: u32,
    pub last_message_timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub achievements: Vec<String>,
    #[serde(default = "default_rank")]
    pub best_rank: u32,
    #[serde(default = "default_rank")]
    pub previous_rank: u32,
    #[serde(default)]
    pub rank_improvement: u32,
    #[serde(default)]
    pub images_shared: u64,
    #[serde(default)]
    pub long_messages: u64,
    #[serde(default)]
    pub links_shared: u64,
    #[serde(default)]
    pub goals_completed: u64,
    #[serde(default)]
    pub boost_days: u64,
    #[serde(default)]
    pub first_boost_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub xp_history: VecDeque<XpEvent>,
}

fn default_rank() -> u32 {
    999
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XpEvent {
    pub amount: u64,
    pub source: String,
    pub note: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyGoal {
    pub date: String, // ISO date
    pub target: u64,
    pub progress: u64,
    pub claimers: Vec<u64>,
    pub completed: bool,
    pub bonus_awarded_to: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageContentStats {
    pub has_image: bool,
    pub is_long: bool,
    pub has_link: bool,
}

impl UserProfile {
    /// Create a default profile with the required ids
    pub fn default_with_ids(user_id: u64, guild_id: u64) -> Self {
        UserProfile {
            user_id,
            guild_id,
            level: 1,
            total_xp: 0,
            xp_to_next_level: 0,
            total_commands_used: 0,
            total_messages: 0,
            last_daily: None,
            daily_streak: 0,
            last_message_timestamp: None,
            achievements: Vec::new(),
            best_rank: 999,
            previous_rank: 999,
            rank_improvement: 0,
            images_shared: 0,
            long_messages: 0,
            links_shared: 0,
            goals_completed: 0,
            boost_days: 0,
            first_boost_date: None,
            xp_history: VecDeque::new(),
        }
    }
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

    /// Get a user's full profile. If the user does not exist, return Ok(None).
    async fn get_user_profile(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<UserProfile>, LevelingError>;

    /// Save a user's profile (upsert semantics).
    async fn save_user_profile(&self, profile: UserProfile) -> Result<(), LevelingError>;

    /// Get all user profiles for a guild (used to calculate leaderboard/rankings)
    async fn get_all_profiles(&self, guild_id: u64) -> Result<Vec<UserProfile>, LevelingError>;

    /// Daily goal per-guild: get and set
    async fn get_daily_goal(&self, guild_id: u64) -> Result<Option<DailyGoal>, LevelingError>;
    async fn save_daily_goal(&self, guild_id: u64, goal: DailyGoal) -> Result<(), LevelingError>;
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

    /// Runtime configuration for XP rolls and cooldowns.
    config: LevelingConfig,
}

/// Configuration knobs for the leveling service.
#[derive(Debug, Clone)]
pub struct LevelingConfig {
    /// Minimum XP a single message can grant.
    pub xp_per_message_min: u64,
    /// Maximum XP a single message can grant.
    pub xp_per_message_max: u64,
    /// Cooldown enforced between message-based XP grants.
    pub cooldown: Duration,
}

impl LevelingConfig {
    #[allow(dead_code)]
    pub fn new(xp_per_message_min: u64, xp_per_message_max: u64, cooldown: Duration) -> Self {
        debug_assert!(xp_per_message_min > 0, "XP minimum must be positive");
        debug_assert!(xp_per_message_max >= xp_per_message_min);

        Self {
            xp_per_message_min,
            xp_per_message_max,
            cooldown,
        }
    }
}

impl Default for LevelingConfig {
    fn default() -> Self {
        // Mirrors the Python implementation's XP roll: 60 second cooldown between XP awards.
        Self {
            xp_per_message_min: 15,
            xp_per_message_max: 25,
            cooldown: Duration::from_secs(60),
        }
    }
}

impl<S: XpStore> LevelingService<S> {
    /// Create a new leveling service with the given storage implementation.
    ///
    /// **Dependency Injection:**
    /// We pass in the storage implementation rather than creating it here.
    /// This is a key principle of Clean Architecture.
    pub fn new(store: S) -> Self {
        Self::with_config(store, LevelingConfig::default())
    }

    /// Create a leveling service with a custom configuration.
    pub fn with_config(store: S, config: LevelingConfig) -> Self {
        Self { store, config }
    }

    /// Maximum number of XP events to keep in history for analytics
    const XP_HISTORY_LIMIT: usize = 120;
    /// Base daily reward
    const BASE_DAILY_REWARD: u64 = 25;
    const STREAK_BONUS_STEP: u64 = 5;
    const STREAK_BONUS_CAP: u64 = 25;
    const GOAL_BONUS_XP: u64 = 15;

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
        boosted: bool,
        content_stats: Option<MessageContentStats>,
    ) -> Result<Option<LevelUpEvent>, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;

        // Load or create profile
        let mut profile = match self.store.get_user_profile(user_id, guild_id).await? {
            Some(p) => p,
            None => self.create_default_profile(user_id, guild_id),
        };

        // Update content stats
        if let Some(stats) = content_stats {
            profile.total_messages = profile.total_messages.saturating_add(1);
            if stats.has_image {
                profile.images_shared = profile.images_shared.saturating_add(1);
            }
            if stats.is_long {
                profile.long_messages = profile.long_messages.saturating_add(1);
            }
            if stats.has_link {
                profile.links_shared = profile.links_shared.saturating_add(1);
            }
        } else {
            // Fallback if not provided (legacy calls)
            profile.total_messages = profile.total_messages.saturating_add(1);
        }

        // Cooldown based on last_message_timestamp if present
        if let Some(last_ts) = profile.last_message_timestamp {
            let now = Utc::now();
            let elapsed = now
                .signed_duration_since(last_ts)
                .to_std()
                .unwrap_or_default();
            if elapsed < self.config.cooldown {
                // Even if on cooldown, we save the profile because we updated message counts
                self.store.save_user_profile(profile).await?;
                let remaining = self.config.cooldown - elapsed;
                return Err(LevelingError::OnCooldown(remaining));
            }
        }

        let old_level = profile.level;

        // Award XP
        let base_gain = self.roll_message_xp();
        let xp_gain = self.apply_xp_boost(base_gain, boosted);
        profile.total_xp = profile.total_xp.saturating_add(xp_gain);
        profile.last_message_timestamp = Some(Utc::now());
        self.record_xp_event(&mut profile, xp_gain, "message".to_string(), None);

        // Check achievements first (they may award bonus XP)
        let _newly_earned = self.check_and_award_achievements_internal(&mut profile);

        // Handle level up
        let leveled_up = self.handle_level_up_internal(&mut profile);

        // Persist changes
        self.store.save_user_profile(profile.clone()).await?;

        if leveled_up {
            Ok(Some(LevelUpEvent {
                user_id,
                guild_id,
                old_level,
                new_level: profile.level,
                total_xp: profile.total_xp,
            }))
        } else {
            Ok(None)
        }
    }

    /// Increment command usage count and check for achievements.
    pub async fn increment_command_count(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<LevelUpEvent>, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;
        let mut profile = match self.store.get_user_profile(user_id, guild_id).await? {
            Some(p) => p,
            None => self.create_default_profile(user_id, guild_id),
        };

        profile.total_commands_used += 1;

        // Check achievements
        let _newly_earned = self.check_and_award_achievements_internal(&mut profile);
        let old_level = profile.level;
        let leveled_up = self.handle_level_up_internal(&mut profile);

        self.store.save_user_profile(profile.clone()).await?;

        if leveled_up {
            Ok(Some(LevelUpEvent {
                user_id,
                guild_id,
                old_level,
                new_level: profile.level,
                total_xp: profile.total_xp,
            }))
        } else {
            Ok(None)
        }
    }

    /// Update booster status for a user.
    pub async fn update_boost_status(
        &self,
        user_id: u64,
        guild_id: u64,
        is_boosting: bool,
    ) -> Result<(), LevelingError> {
        Self::validate_ids(user_id, guild_id)?;
        let mut profile = match self.store.get_user_profile(user_id, guild_id).await? {
            Some(p) => p,
            None => self.create_default_profile(user_id, guild_id),
        };

        if is_boosting {
            if profile.first_boost_date.is_none() {
                profile.first_boost_date = Some(Utc::now());
            }
            if let Some(first_date) = profile.first_boost_date {
                let days = (Utc::now() - first_date).num_days();
                profile.boost_days = days.max(0) as u64;
            }
        } else if profile.first_boost_date.is_some() {
            profile.first_boost_date = None;
            profile.boost_days = 0;
        }

        // Check achievements (e.g. booster badge)
        self.check_and_award_achievements_internal(&mut profile);

        self.store.save_user_profile(profile).await?;
        Ok(())
    }

    /// Get the next closest achievement for the user.
    pub fn get_next_achievement(
        &self,
        profile: &UserProfile,
    ) -> Option<(Achievement, f64, u64, u64)> {
        let all_achievements = get_all_achievements();
        let mut candidates = Vec::new();

        for achievement in all_achievements {
            if profile.achievements.contains(&achievement.id) {
                continue;
            }

            let (current, target, progress_override) = match achievement.id.as_str() {
                // Level milestones
                "first_steps" => (profile.level as u64, 5, None),
                "rising_star" => (profile.level as u64, 10, None),
                "veteran" => (profile.level as u64, 25, None),
                "legend" => (profile.level as u64, 50, None),
                "centurion" => (profile.level as u64, 100, None),
                "halfway_there" => (profile.level as u64, 15, None),

                // Message milestones
                "chatterbox" => (profile.total_messages, 100, None),
                "conversationalist" => (profile.total_messages, 500, None),
                "voice_of_the_server" => (profile.total_messages, 1000, None),
                "veteran_speaker" => (profile.total_messages, 5000, None),

                // Command usage
                "command_novice" => (profile.total_commands_used, 25, None),
                "command_expert" => (profile.total_commands_used, 100, None),
                "command_master" => (profile.total_commands_used, 500, None),

                // Daily streak achievements
                "streak_starter" => (profile.daily_streak as u64, 3, None),
                "week_warrior" => (profile.daily_streak as u64, 7, None),
                "biweekly_dedication" => (profile.daily_streak as u64, 14, None),
                "month_master" => (profile.daily_streak as u64, 30, None),
                "dedication_deity" => (profile.daily_streak as u64, 100, None),
                "half_year_hero" => (profile.daily_streak as u64, 180, None),
                "yearly_champion" => (profile.daily_streak as u64, 365, None),

                // XP milestones
                "xp_collector" => (profile.total_xp, 1000, None),
                "xp_hoarder" => (profile.total_xp, 5000, None),
                "xp_tycoon" => (profile.total_xp, 10000, None),
                "xp_millionaire" => (profile.total_xp, 25000, None),

                // Special achievements
                "early_bird" => (profile.last_daily.is_some() as u64, 1, None),
                "booster_badge" => (profile.boost_days, 1, None),
                "server_supporter" => (profile.boost_days, 30, None),
                "well_rounded" => {
                    // Combine requirements by taking the minimum percentage across all gates.
                    let level_progress = profile.level as f64 / 10.0;
                    let message_progress = profile.total_messages as f64 / 500.0;
                    let command_progress = profile.total_commands_used as f64 / 50.0;
                    let progress = level_progress
                        .min(message_progress)
                        .min(command_progress)
                        .min(1.0);
                    // Represent as x/100 so the bar shows percent complete.
                    ((progress * 100.0) as u64, 100, Some(progress))
                }

                // Content Creator
                "photographer" => (profile.images_shared, 50, None),
                "lengthy_talker" => (profile.long_messages, 50, None),
                "link_sharer" => (profile.links_shared, 50, None),

                // Server Participation
                "goal_contributor" => (profile.goals_completed, 10, None),
                "goal_enthusiast" => (profile.goals_completed, 50, None),

                // Leaderboard & Competition (lower rank is better, so invert the fraction)
                "podium_finish" => {
                    let rank = profile.best_rank.max(1) as f64;
                    let progress = (3.0 / rank).min(1.0);
                    (profile.best_rank as u64, 3, Some(progress))
                }
                "top_ten" => {
                    let rank = profile.best_rank.max(1) as f64;
                    let progress = (10.0 / rank).min(1.0);
                    (profile.best_rank as u64, 10, Some(progress))
                }
                "leaderboard_climber" => (profile.rank_improvement as u64, 10, None),

                // Meta
                "achievement_hunter" => (profile.achievements.len() as u64, 10, None),
                "completionist" => (profile.achievements.len() as u64, 30, None),

                _ => (0, 0, None),
            };

            if target > 0 {
                let progress =
                    progress_override.unwrap_or_else(|| (current as f64 / target as f64).min(1.0));
                candidates.push((achievement, progress, current, target));
            }
        }

        // Sort by progress descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.into_iter().next()
    }

    /// Create a default user profile (when a user has no existing data)
    fn create_default_profile(&self, user_id: u64, guild_id: u64) -> UserProfile {
        UserProfile {
            user_id,
            guild_id,
            level: 1,
            total_xp: 0,
            xp_to_next_level: Self::xp_threshold_for_level(2),
            total_commands_used: 0,
            total_messages: 0,
            last_daily: None,
            daily_streak: 0,
            last_message_timestamp: None,
            achievements: Vec::new(),
            best_rank: 999,
            previous_rank: 999,
            rank_improvement: 0,
            images_shared: 0,
            long_messages: 0,
            links_shared: 0,
            goals_completed: 0,
            boost_days: 0,
            first_boost_date: None,
            xp_history: VecDeque::new(),
        }
    }

    fn record_xp_event(
        &self,
        profile: &mut UserProfile,
        amount: u64,
        source: String,
        note: Option<String>,
    ) {
        if amount == 0 {
            return;
        }
        let event = XpEvent {
            amount,
            source,
            note,
            timestamp: Utc::now(),
        };
        profile.xp_history.push_back(event);
        while profile.xp_history.len() > Self::XP_HISTORY_LIMIT {
            profile.xp_history.pop_front();
        }
    }

    /// Apply XP boost multiplier (e.g., Nitro boosters). The discord layer will pass boosted=false/true.
    fn apply_xp_boost(&self, base_xp: u64, boosted: bool) -> u64 {
        if boosted {
            let multiplier = 1.5_f64;
            ((base_xp as f64) * multiplier).round() as u64
        } else {
            base_xp
        }
    }

    /// Check and award achievements (internal simplified version). Returns list of newly earned IDs.
    fn check_and_award_achievements_internal(&self, profile: &mut UserProfile) -> Vec<String> {
        let mut newly = Vec::new();
        let all_achievements = get_all_achievements();

        for achievement in all_achievements {
            if profile.achievements.contains(&achievement.id) {
                continue;
            }

            let meets = match achievement.id.as_str() {
                // Level milestones
                "first_steps" => profile.level >= 5,
                "rising_star" => profile.level >= 10,
                "veteran" => profile.level >= 25,
                "legend" => profile.level >= 50,
                "centurion" => profile.level >= 100,
                "halfway_there" => profile.level >= 15,

                // Message milestones
                "chatterbox" => profile.total_messages >= 100,
                "conversationalist" => profile.total_messages >= 500,
                "voice_of_the_server" => profile.total_messages >= 1000,
                "veteran_speaker" => profile.total_messages >= 5000,

                // Command usage
                "command_novice" => profile.total_commands_used >= 25,
                "command_expert" => profile.total_commands_used >= 100,
                "command_master" => profile.total_commands_used >= 500,

                // Daily streak achievements
                "streak_starter" => profile.daily_streak >= 3,
                "week_warrior" => profile.daily_streak >= 7,
                "biweekly_dedication" => profile.daily_streak >= 14,
                "month_master" => profile.daily_streak >= 30,
                "dedication_deity" => profile.daily_streak >= 100,
                "half_year_hero" => profile.daily_streak >= 180,
                "yearly_champion" => profile.daily_streak >= 365,

                // XP milestones
                "xp_collector" => profile.total_xp >= 1000,
                "xp_hoarder" => profile.total_xp >= 5000,
                "xp_tycoon" => profile.total_xp >= 10000,
                "xp_millionaire" => profile.total_xp >= 25000,

                // Special achievements
                "early_bird" => profile.last_daily.is_some(),
                "booster_badge" => profile.boost_days > 0,
                "server_supporter" => profile.boost_days >= 30,
                "well_rounded" => {
                    profile.level >= 10
                        && profile.total_messages >= 500
                        && profile.total_commands_used >= 50
                }

                // Leaderboard & Competition
                "podium_finish" => profile.best_rank <= 3,
                "top_ten" => profile.best_rank <= 10,
                "leaderboard_climber" => profile.rank_improvement >= 10,

                // Content Creator
                "photographer" => profile.images_shared >= 50,
                "lengthy_talker" => profile.long_messages >= 50,
                "link_sharer" => profile.links_shared >= 50,

                // Server Participation
                "goal_contributor" => profile.goals_completed >= 10,
                "goal_enthusiast" => profile.goals_completed >= 50,

                // Meta
                "achievement_hunter" => profile.achievements.len() >= 10,
                "completionist" => profile.achievements.len() >= 30,

                _ => false,
            };

            if meets {
                profile.achievements.push(achievement.id.clone());
                profile.total_xp = profile.total_xp.saturating_add(achievement.reward_xp);
                self.record_xp_event(
                    profile,
                    achievement.reward_xp,
                    "achievement".to_string(),
                    Some(achievement.name.clone()),
                );
                newly.push(achievement.id.clone());
            }
        }

        newly
    }

    /// Internal handler for leveling up a user's profile. Returns true if leveled.
    fn handle_level_up_internal(&self, profile: &mut UserProfile) -> bool {
        let mut leveled = false;
        while profile.total_xp >= Self::xp_threshold_for_level(profile.level + 1) {
            profile.level += 1;
            leveled = true;
        }
        profile.xp_to_next_level = Self::xp_threshold_for_level(profile.level + 1);
        leveled
    }

    /// Calculate level from total XP using the legacy Python curve (100 * (level-1)^1.5).
    pub fn calculate_level(&self, xp: u64) -> u32 {
        Self::level_from_xp(xp)
    }

    /// Static helper so other layers (like infra) can reuse the level curve math.
    pub fn level_from_xp(xp: u64) -> u32 {
        if xp == 0 {
            return 1;
        }

        let approx = ((xp as f64 / 100.0).powf(2.0 / 3.0)).floor() as u32 + 1;
        let mut level = approx.max(1);

        // Adjust upward if we undershot.
        while level < u32::MAX && xp >= Self::xp_threshold_for_level(level + 1) {
            level += 1;
        }

        // Adjust downward if we overshot (can happen near boundaries due to float math).
        while level > 1 && xp < Self::xp_threshold_for_level(level) {
            level -= 1;
        }

        level
    }

    /// Total XP required to REACH the next level (inclusive of previous levels).
    pub fn xp_for_next_level(&self, current_level: u32) -> u64 {
        Self::xp_threshold_for_level(current_level + 1)
    }

    /// Total XP required to reach the provided level.
    pub fn xp_for_level(&self, level: u32) -> u64 {
        Self::xp_threshold_for_level(level)
    }

    fn xp_threshold_for_level(level: u32) -> u64 {
        if level <= 1 {
            return 0;
        }

        let power = (level - 1) as f64;
        (100.0 * power.powf(1.5)) as u64
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

    /// Get the full user profile (contains historic and meta data)
    pub async fn get_user_profile(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<UserProfile, LevelingError> {
        Self::validate_ids(user_id, guild_id)?;
        if let Some(profile) = self.store.get_user_profile(user_id, guild_id).await? {
            Ok(profile)
        } else {
            Ok(self.create_default_profile(user_id, guild_id))
        }
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

    /// Recalculate ranks for all profiles in a guild and persist the updated rank fields.
    /// Returns the profiles sorted by rank (highest XP first).
    pub async fn recalculate_and_update_ranks(
        &self,
        guild_id: u64,
    ) -> Result<Vec<UserProfile>, LevelingError> {
        Self::validate_guild_id(guild_id)?;
        let mut profiles = self.store.get_all_profiles(guild_id).await?;
        // Sort by total_xp desc
        profiles.sort_by(|a, b| b.total_xp.cmp(&a.total_xp));

        for (index, profile) in profiles.iter_mut().enumerate() {
            let rank = (index + 1) as u32;
            let previous_rank = profile.previous_rank;
            let best_rank = profile.best_rank;
            // Update best rank if improved
            if rank < best_rank {
                profile.best_rank = rank;
            }
            // Update rank improvement
            if previous_rank != 999 && previous_rank > rank {
                let improvement = previous_rank - rank;
                if improvement > profile.rank_improvement {
                    profile.rank_improvement = improvement;
                }
            }
            profile.previous_rank = rank;

            // Save profile back
            self.store.save_user_profile(profile.clone()).await?;
        }

        Ok(profiles)
    }

    /// Claim the daily reward for a user. Returns the amount of XP awarded and whether the user leveled up.
    pub async fn claim_daily(
        &self,
        user_id: u64,
        guild_id: u64,
        boosted: bool,
        member_count: u64,
    ) -> Result<(u64, Option<LevelUpEvent>), LevelingError> {
        Self::validate_ids(user_id, guild_id)?;

        let mut profile = match self.store.get_user_profile(user_id, guild_id).await? {
            Some(p) => p,
            None => self.create_default_profile(user_id, guild_id),
        };

        let now = chrono::Utc::now();
        let today = now.date_naive();
        let last_daily_date = profile.last_daily.map(|d| d.date_naive());

        if let Some(last) = last_daily_date {
            if last == today {
                // Already claimed
                return Ok((0, None));
            }
        }

        // Update streak
        let streak = match last_daily_date {
            Some(last) => {
                let delta_days = (today - last).num_days();
                if delta_days == 1 {
                    profile.daily_streak += 1;
                } else {
                    profile.daily_streak = 1;
                }
                profile.daily_streak
            }
            None => {
                profile.daily_streak = 1;
                profile.daily_streak
            }
        };

        let streak_bonus = std::cmp::min(
            (streak.saturating_sub(1) as u64).saturating_mul(Self::STREAK_BONUS_STEP),
            Self::STREAK_BONUS_CAP,
        );
        let base_daily_xp = Self::BASE_DAILY_REWARD + streak_bonus;
        let award_xp = self.apply_xp_boost(base_daily_xp, boosted);

        profile.total_xp = profile.total_xp.saturating_add(award_xp);
        profile.last_daily = Some(now);
        let streak_note = format!("streak {}d", profile.daily_streak);
        self.record_xp_event(
            &mut profile,
            award_xp,
            "daily".to_string(),
            Some(streak_note),
        );

        // Persist and check level up
        let old_level = profile.level;
        let leveled = self.handle_level_up_internal(&mut profile);
        self.store.save_user_profile(profile.clone()).await?;

        // Now handle the server-wide daily goal
        let mut daily_goal = match self.store.get_daily_goal(guild_id).await? {
            Some(g) => g,
            None => DailyGoal {
                date: now.date_naive().to_string(),
                target: self.calculate_daily_goal_target(member_count),
                progress: 0,
                claimers: vec![],
                completed: false,
                bonus_awarded_to: vec![],
            },
        };

        // If the stored goal has a different date, reset
        if daily_goal.date != now.date_naive().to_string() {
            daily_goal = DailyGoal {
                date: now.date_naive().to_string(),
                target: self.calculate_daily_goal_target(member_count),
                progress: 0,
                claimers: vec![],
                completed: false,
                bonus_awarded_to: vec![],
            };
        }

        // Add claimer if not present
        if !daily_goal.claimers.contains(&user_id) {
            daily_goal.claimers.push(user_id);
            daily_goal.progress = daily_goal.claimers.len() as u64;
        }

        let mut user_goal_bonus = 0_u64;
        let mut _goal_completion_message: Option<String> = None;

        if !daily_goal.completed && daily_goal.progress >= daily_goal.target {
            daily_goal.completed = true;
            // We'll save profile once after applying any goal bonuses
        }

        if daily_goal.completed {
            let mut newly_awarded: Vec<u64> = Vec::new();
            for claimer_id in daily_goal.claimers.clone() {
                if !daily_goal.bonus_awarded_to.contains(&claimer_id) {
                    // award
                    let mut claimer_profile =
                        match self.store.get_user_profile(claimer_id, guild_id).await? {
                            Some(p) => p,
                            None => self.create_default_profile(claimer_id, guild_id),
                        };
                    claimer_profile.total_xp =
                        claimer_profile.total_xp.saturating_add(Self::GOAL_BONUS_XP);
                    self.record_xp_event(
                        &mut claimer_profile,
                        Self::GOAL_BONUS_XP,
                        "goal_bonus".to_string(),
                        Some(daily_goal.date.clone()),
                    );
                    claimer_profile.goals_completed =
                        claimer_profile.goals_completed.saturating_add(1);
                    // Recompute level after awarding bonus if applicable
                    let _leveled = self.handle_level_up_internal(&mut claimer_profile);
                    // Save claimer profile
                    self.store
                        .save_user_profile(claimer_profile.clone())
                        .await?;
                    daily_goal.bonus_awarded_to.push(claimer_id);
                    newly_awarded.push(claimer_id);
                    if claimer_id == user_id {
                        user_goal_bonus = Self::GOAL_BONUS_XP;
                    }
                }
            }
        }

        self.store.save_daily_goal(guild_id, daily_goal).await?;

        let total_award = award_xp + user_goal_bonus;
        if leveled {
            Ok((
                total_award,
                Some(LevelUpEvent {
                    user_id,
                    guild_id,
                    old_level,
                    new_level: profile.level,
                    total_xp: profile.total_xp + user_goal_bonus,
                }),
            ))
        } else {
            Ok((total_award, None))
        }
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

        // Use full profile so we record a rich XP event and run achievement checks.
        let mut profile = match self.store.get_user_profile(user_id, guild_id).await? {
            Some(p) => p,
            None => self.create_default_profile(user_id, guild_id),
        };

        let old_level = profile.level;

        profile.total_xp = profile.total_xp.saturating_add(total_amount);
        // Record detailed event for analytics
        let source_label = match &source {
            XpSource::Message => "message".to_string(),
            XpSource::VoiceMinute => "voice_minute".to_string(),
            XpSource::CodeChallenge {
                difficulty,
                language,
                execution_time_ms,
            } => {
                format!(
                    "code_{} ({}) {}ms",
                    format!("{:?}", difficulty).to_lowercase(),
                    language,
                    execution_time_ms
                )
            }
        };
        self.record_xp_event(&mut profile, total_amount, source_label, None);

        // Check achievements (these may award additional XP and update profile)
        let _new_ach = self.check_and_award_achievements_internal(&mut profile);

        // Handle level up (recomputes xp_to_next_level)
        let leveled = self.handle_level_up_internal(&mut profile);

        // Save profile back to the store
        self.store.save_user_profile(profile.clone()).await?;

        if leveled {
            Ok(Some(LevelUpEvent {
                user_id,
                guild_id,
                old_level,
                new_level: profile.level,
                total_xp: profile.total_xp,
            }))
        } else {
            Ok(None)
        }
    }

    fn roll_message_xp(&self) -> u64 {
        if self.config.xp_per_message_min == self.config.xp_per_message_max {
            return self.config.xp_per_message_min;
        }

        let mut rng = rand::thread_rng();
        rng.gen_range(self.config.xp_per_message_min..=self.config.xp_per_message_max)
    }

    /// Determine the daily goal target for the guild. Default to 15 claims.
    pub fn calculate_daily_goal_target(&self, _member_count: u64) -> u64 {
        // Keep it simple: a small guild needs fewer people (member_count min 1), max 15
        let members = _member_count.max(1);
        std::cmp::min(15, members)
    }

    /// Get or create the current daily goal state for the guild.
    pub async fn get_daily_goal_state(
        &self,
        guild_id: u64,
        member_count: u64,
    ) -> Result<DailyGoal, LevelingError> {
        Self::validate_guild_id(guild_id)?;
        let now = chrono::Utc::now();
        let mut daily_goal = self
            .store
            .get_daily_goal(guild_id)
            .await?
            .unwrap_or(DailyGoal {
                date: now.date_naive().to_string(),
                target: self.calculate_daily_goal_target(member_count),
                progress: 0,
                claimers: vec![],
                completed: false,
                bonus_awarded_to: vec![],
            });
        if daily_goal.date != now.date_naive().to_string() {
            daily_goal = DailyGoal {
                date: now.date_naive().to_string(),
                target: self.calculate_daily_goal_target(member_count),
                progress: 0,
                claimers: vec![],
                completed: false,
                bonus_awarded_to: vec![],
            };
            self.store
                .save_daily_goal(guild_id, daily_goal.clone())
                .await?;
        }
        Ok(daily_goal)
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

        async fn get_user_profile(
            &self,
            _: u64,
            _: u64,
        ) -> Result<Option<UserProfile>, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn save_user_profile(&self, _: UserProfile) -> Result<(), LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn get_all_profiles(&self, _: u64) -> Result<Vec<UserProfile>, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn get_daily_goal(&self, _: u64) -> Result<Option<DailyGoal>, LevelingError> {
            Err(LevelingError::StorageError(
                "Noop store should not be used".to_string(),
            ))
        }

        async fn save_daily_goal(&self, _: u64, _: DailyGoal) -> Result<(), LevelingError> {
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
        assert_eq!(service.calculate_level(100), 2);
        assert_eq!(service.calculate_level(200), 2);
        assert_eq!(service.calculate_level(450), 3);
    }

    #[test]
    fn test_xp_for_next_level() {
        let service = make_service();

        assert_eq!(service.xp_for_next_level(1), 100);
        assert_eq!(service.xp_for_next_level(2), 282); // floor(100 * 2^1.5)
        assert_eq!(service.xp_for_next_level(3), 519); // floor(100 * 3^1.5)
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

    #[tokio::test]
    async fn test_claim_daily_awards_xp_and_streaks() {
        // Use the real in-memory store for behavior
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);

        let user_id = 1u64;
        let guild_id = 1u64;

        // First claim
        let (xp, levelup) = service
            .claim_daily(user_id, guild_id, false, 1)
            .await
            .unwrap();
        assert!(
            xp >= LevelingService::<crate::infra::leveling::InMemoryXpStore>::BASE_DAILY_REWARD
        );
        assert!(levelup.is_none());

        // Second claim same day should return 0
        let (xp2, _) = service
            .claim_daily(user_id, guild_id, false, 1)
            .await
            .unwrap();
        assert_eq!(xp2, 0);
    }

    #[tokio::test]
    async fn test_process_message_cooldown() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);
        let user_id = 100u64;
        let guild_id = 11u64;

        // First message should award XP
        let res = service
            .process_message(user_id, guild_id, false, None)
            .await;
        assert!(res.is_ok());

        // Second message within cooldown should return OnCooldown
        let res2 = service
            .process_message(user_id, guild_id, false, None)
            .await;
        assert!(matches!(res2, Err(LevelingError::OnCooldown(_))));
    }

    #[tokio::test]
    async fn test_process_message_boost_multiplier() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let mut config = LevelingConfig::default();
        // Use a fixed roll so we can test exact values
        config.xp_per_message_min = 10;
        config.xp_per_message_max = 10;
        config.cooldown = Duration::from_secs(0);
        let service = LevelingService::with_config(store, config);

        let user_id = 999u64;
        let guild_id = 9u64;

        // Non-boosted message
        let res = service
            .process_message(user_id, guild_id, false, None)
            .await
            .unwrap();
        assert!(res.is_none()); // 10 XP shouldn't reach a new level

        // Boosted message should give 15 XP instead of 10
        let res2 = service
            .process_message(user_id, guild_id, true, None)
            .await
            .unwrap();
        assert!(res2.is_none());
        let profile = service.get_user_profile(user_id, guild_id).await.unwrap();
        assert!(profile.total_xp >= 25);
    }

    #[tokio::test]
    async fn test_award_xp_records_event_and_achievements() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);
        let user_id = 33u64;
        let guild_id = 12u64;

        // Award XP via award_xp; ensure xp_history gets an event and achievements may be awarded
        let _ = service
            .award_xp(user_id, guild_id, 1000, XpSource::Message)
            .await
            .unwrap();
        // Level change or no is not important; check profile
        let profile = service.get_user_profile(user_id, guild_id).await.unwrap();
        assert!(profile.total_xp >= 1000);
        assert!(!profile.xp_history.is_empty());
        // xp_collector achievement requires 1000 total XP
        assert!(profile.achievements.iter().any(|id| id == "xp_collector"));
    }

    #[tokio::test]
    async fn test_increment_command_count_achievements() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);
        let user_id = 55u64;
        let guild_id = 22u64;

        // Increment command count 25 times to earn command_novice
        for _ in 0..25 {
            let _ = service.increment_command_count(user_id, guild_id).await;
        }

        let profile = service.get_user_profile(user_id, guild_id).await.unwrap();
        assert!(profile.total_commands_used >= 25);
        assert!(profile.achievements.iter().any(|id| id == "command_novice"));
    }

    #[tokio::test]
    async fn test_message_content_stats_counters() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);
        let user_id = 66u64;
        let guild_id = 33u64;

        let stats = MessageContentStats {
            has_image: true,
            is_long: true,
            has_link: true,
        };
        let _ = service
            .process_message(user_id, guild_id, false, Some(stats))
            .await
            .unwrap();

        let profile = service.get_user_profile(user_id, guild_id).await.unwrap();
        assert_eq!(profile.images_shared, 1);
        assert_eq!(profile.long_messages, 1);
        assert_eq!(profile.links_shared, 1);
    }

    #[tokio::test]
    async fn test_claim_daily_awards_goal_bonus() {
        let store = crate::infra::leveling::InMemoryXpStore::new();
        let service = LevelingService::new(store);

        let user_id = 99u64;
        let guild_id = 11u64;

        // member_count=1 -> target = 1, should cause immediate completion and award goal bonus
        let (xp, _levelup) = service
            .claim_daily(user_id, guild_id, false, 1)
            .await
            .unwrap();
        // xp must include base daily reward and the goal bonus
        assert!(
            xp >= LevelingService::<crate::infra::leveling::InMemoryXpStore>::BASE_DAILY_REWARD
                + LevelingService::<crate::infra::leveling::InMemoryXpStore>::GOAL_BONUS_XP
        );
        // Verify profile reflects bonus
        let profile = service.get_user_profile(user_id, guild_id).await.unwrap();
        assert!(
            profile.total_xp
                >= LevelingService::<crate::infra::leveling::InMemoryXpStore>::BASE_DAILY_REWARD
                    + LevelingService::<crate::infra::leveling::InMemoryXpStore>::GOAL_BONUS_XP
        );
    }
}
