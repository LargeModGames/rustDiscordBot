// Economy system core - business logic for GreyCoins
//
// This module contains all the domain logic for the economy system.
// Following the same pattern as the leveling system, this is platform-agnostic
// with no Discord-specific code.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// DOMAIN MODELS
// ============================================================================

/// Represents a user's wallet in a specific guild.
#[derive(Debug, Clone)]
pub struct Wallet {
    #[allow(dead_code)]
    pub user_id: u64,
    #[allow(dead_code)]
    pub guild_id: u64,
    pub balance: i64,
    pub last_daily: Option<DateTime<Utc>>,
    pub total_earned: i64,
}

/// Represents a coin transaction for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub user_id: u64,
    pub guild_id: u64,
    pub amount: i64,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

/// Result of a daily claim attempt.
#[derive(Debug, Clone)]
pub struct DailyClaimResult {
    pub coins_awarded: i64,
    pub new_balance: i64,
    #[allow(dead_code)]
    pub next_claim_time: DateTime<Utc>,
}

// ============================================================================
// ERRORS
// ============================================================================

#[derive(Debug, Clone)]
pub enum EconomyError {
    #[allow(dead_code)]
    InsufficientFunds {
        required: i64,
        available: i64,
    },
    #[allow(dead_code)]
    OnCooldown {
        available_at: DateTime<Utc>,
    },
    StoreError(String),
}

impl fmt::Display for EconomyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EconomyError::InsufficientFunds {
                required,
                available,
            } => {
                write!(
                    f,
                    "Insufficient funds: need {} coins, but only have {}",
                    required, available
                )
            }
            EconomyError::OnCooldown { available_at } => {
                write!(f, "On cooldown until {}", available_at)
            }
            EconomyError::StoreError(msg) => write!(f, "Store error: {}", msg),
        }
    }
}

impl std::error::Error for EconomyError {}

// ============================================================================
// STORAGE TRAIT
// ============================================================================

/// Trait for persisting economy data.
///
/// This abstraction allows different implementations (in-memory for testing,
/// SQLite for production) following the Dependency Inversion Principle.
#[async_trait]
pub trait CoinStore: Send + Sync {
    /// Get a user's wallet, creating it if it doesn't exist.
    async fn get_wallet(&self, user_id: u64, guild_id: u64) -> Result<Wallet, EconomyError>;

    /// Update a user's balance.
    #[allow(dead_code)]
    async fn update_balance(
        &self,
        user_id: u64,
        guild_id: u64,
        new_balance: i64,
    ) -> Result<(), EconomyError>;

    /// Update the last daily claim time.
    async fn update_last_daily(
        &self,
        user_id: u64,
        guild_id: u64,
        timestamp: DateTime<Utc>,
    ) -> Result<(), EconomyError>;

    /// Add coins and update total_earned counter.
    async fn add_coins(&self, user_id: u64, guild_id: u64, amount: i64)
        -> Result<(), EconomyError>;

    /// Log a transaction for audit trail.
    async fn log_transaction(&self, transaction: Transaction) -> Result<(), EconomyError>;

    /// Get recent transactions for a user.
    async fn get_transactions(
        &self,
        user_id: u64,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<Transaction>, EconomyError>;
}

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for the economy system.
#[derive(Debug, Clone)]
pub struct EconomyConfig {
    /// How many coins to award for daily claim.
    pub daily_reward: i64,

    /// Cooldown period for daily claims (in hours).
    pub daily_cooldown_hours: i64,

    /// Chance (0.0 to 1.0) to award coins on message.
    pub message_reward_chance: f64,

    /// Minimum coins to award on random message reward.
    pub message_reward_min: i64,

    /// Maximum coins to award on random message reward.
    pub message_reward_max: i64,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            daily_reward: 10,
            daily_cooldown_hours: 24,
            message_reward_chance: 0.05, // 5%
            message_reward_min: 1,
            message_reward_max: 5,
        }
    }
}

// ============================================================================
// CORE SERVICE
// ============================================================================

/// The main service for economy operations.
///
/// Generic over S: CoinStore so we can swap implementations.
pub struct EconomyService<S: CoinStore> {
    store: S,
    config: EconomyConfig,
}

impl<S: CoinStore> EconomyService<S> {
    /// Create a new economy service with the given store.
    pub fn new(store: S) -> Self {
        Self {
            store,
            config: EconomyConfig::default(),
        }
    }

    /// Create a new economy service with custom configuration.
    #[allow(dead_code)]
    pub fn new_with_config(store: S, config: EconomyConfig) -> Self {
        Self { store, config }
    }

    /// Get a user's current balance.
    pub async fn get_balance(&self, user_id: u64, guild_id: u64) -> Result<i64, EconomyError> {
        let wallet = self.store.get_wallet(user_id, guild_id).await?;
        Ok(wallet.balance)
    }

    /// Get a user's full wallet information.
    pub async fn get_wallet(&self, user_id: u64, guild_id: u64) -> Result<Wallet, EconomyError> {
        self.store.get_wallet(user_id, guild_id).await
    }

    /// Award coins to a user with a reason.
    pub async fn award_coins(
        &self,
        user_id: u64,
        guild_id: u64,
        amount: i64,
        reason: String,
    ) -> Result<i64, EconomyError> {
        if amount <= 0 {
            return Err(EconomyError::StoreError(
                "Amount must be positive".to_string(),
            ));
        }

        // Add coins (this also updates total_earned)
        self.store.add_coins(user_id, guild_id, amount).await?;

        // Log the transaction
        let transaction = Transaction {
            user_id,
            guild_id,
            amount,
            reason,
            timestamp: Utc::now(),
        };
        self.store.log_transaction(transaction).await?;

        // Return new balance
        self.get_balance(user_id, guild_id).await
    }

    /// Attempt to claim daily reward.
    ///
    /// Returns Ok(Some(result)) if claimed successfully.
    /// Returns Ok(None) if on cooldown.
    pub async fn claim_daily(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<DailyClaimResult>, EconomyError> {
        let wallet = self.store.get_wallet(user_id, guild_id).await?;
        let now = Utc::now();

        // Check if on cooldown
        if let Some(last_daily) = wallet.last_daily {
            let next_claim = last_daily + Duration::hours(self.config.daily_cooldown_hours);
            if now < next_claim {
                return Ok(None);
            }
        }

        // Award coins
        let new_balance = self
            .award_coins(
                user_id,
                guild_id,
                self.config.daily_reward,
                "Daily claim".to_string(),
            )
            .await?;

        // Update last daily time
        self.store.update_last_daily(user_id, guild_id, now).await?;

        Ok(Some(DailyClaimResult {
            coins_awarded: self.config.daily_reward,
            new_balance,
            next_claim_time: now + Duration::hours(self.config.daily_cooldown_hours),
        }))
    }

    /// Try to award random coins for a message.
    ///
    /// Returns Some(amount) if coins were awarded, None otherwise.
    pub async fn try_random_message_reward(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<i64>, EconomyError> {
        // Use a Send-safe rng instead of thread_rng
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};
        use std::time::SystemTime;

        // Create a seed from system time and user/guild IDs for variety
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
            ^ user_id
            ^ guild_id;

        let mut rng = StdRng::seed_from_u64(seed);

        if rng.gen::<f64>() < self.config.message_reward_chance {
            // Award random amount between min and max
            let amount =
                rng.gen_range(self.config.message_reward_min..=self.config.message_reward_max);

            self.award_coins(
                user_id,
                guild_id,
                amount,
                "Random message reward".to_string(),
            )
            .await?;

            Ok(Some(amount))
        } else {
            Ok(None)
        }
    }

    /// Get recent transactions for a user.
    pub async fn get_recent_transactions(
        &self,
        user_id: u64,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<Transaction>, EconomyError> {
        self.store.get_transactions(user_id, guild_id, limit).await
    }

    /// Purchase a shop item with coins.
    ///
    /// Deducts the item price from the user's balance and returns the new balance.
    /// Does not add item to inventory - that should be done separately.
    pub async fn deduct_coins_for_purchase(
        &self,
        user_id: u64,
        guild_id: u64,
        amount: i64,
        reason: String,
    ) -> Result<i64, EconomyError> {
        if amount <= 0 {
            return Err(EconomyError::StoreError(
                "Amount must be positive".to_string(),
            ));
        }

        // Check if user has sufficient funds
        let wallet = self.store.get_wallet(user_id, guild_id).await?;
        if wallet.balance < amount {
            return Err(EconomyError::InsufficientFunds {
                required: amount,
                available: wallet.balance,
            });
        }

        // Deduct coins
        let new_balance = wallet.balance - amount;
        self.store
            .update_balance(user_id, guild_id, new_balance)
            .await?;

        // Log the transaction (negative amount to indicate purchase)
        let transaction = Transaction {
            user_id,
            guild_id,
            amount: -amount, // Negative for purchase
            reason,
            timestamp: Utc::now(),
        };
        self.store.log_transaction(transaction).await?;

        Ok(new_balance)
    }

    /// Get the next daily claim time for a user.
    pub async fn get_next_daily_time(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<DateTime<Utc>>, EconomyError> {
        let wallet = self.store.get_wallet(user_id, guild_id).await?;

        Ok(wallet
            .last_daily
            .map(|last| last + Duration::hours(self.config.daily_cooldown_hours)))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // Simple in-memory store for testing
    struct InMemoryCoinStore {
        wallets: Arc<Mutex<HashMap<(u64, u64), Wallet>>>,
        transactions: Arc<Mutex<Vec<Transaction>>>,
    }

    impl InMemoryCoinStore {
        fn new() -> Self {
            Self {
                wallets: Arc::new(Mutex::new(HashMap::new())),
                transactions: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl CoinStore for InMemoryCoinStore {
        async fn get_wallet(&self, user_id: u64, guild_id: u64) -> Result<Wallet, EconomyError> {
            let mut wallets = self.wallets.lock().unwrap();
            Ok(wallets
                .entry((user_id, guild_id))
                .or_insert_with(|| Wallet {
                    user_id,
                    guild_id,
                    balance: 0,
                    last_daily: None,
                    total_earned: 0,
                })
                .clone())
        }

        async fn update_balance(
            &self,
            user_id: u64,
            guild_id: u64,
            new_balance: i64,
        ) -> Result<(), EconomyError> {
            let mut wallets = self.wallets.lock().unwrap();
            if let Some(wallet) = wallets.get_mut(&(user_id, guild_id)) {
                wallet.balance = new_balance;
            }
            Ok(())
        }

        async fn update_last_daily(
            &self,
            user_id: u64,
            guild_id: u64,
            timestamp: DateTime<Utc>,
        ) -> Result<(), EconomyError> {
            let mut wallets = self.wallets.lock().unwrap();
            if let Some(wallet) = wallets.get_mut(&(user_id, guild_id)) {
                wallet.last_daily = Some(timestamp);
            }
            Ok(())
        }

        async fn add_coins(
            &self,
            user_id: u64,
            guild_id: u64,
            amount: i64,
        ) -> Result<(), EconomyError> {
            let mut wallets = self.wallets.lock().unwrap();
            let wallet = wallets
                .entry((user_id, guild_id))
                .or_insert_with(|| Wallet {
                    user_id,
                    guild_id,
                    balance: 0,
                    last_daily: None,
                    total_earned: 0,
                });
            wallet.balance += amount;
            wallet.total_earned += amount;
            Ok(())
        }

        async fn log_transaction(&self, transaction: Transaction) -> Result<(), EconomyError> {
            let mut transactions = self.transactions.lock().unwrap();
            transactions.push(transaction);
            Ok(())
        }

        async fn get_transactions(
            &self,
            user_id: u64,
            guild_id: u64,
            limit: usize,
        ) -> Result<Vec<Transaction>, EconomyError> {
            let transactions = self.transactions.lock().unwrap();
            let filtered: Vec<Transaction> = transactions
                .iter()
                .filter(|t| t.user_id == user_id && t.guild_id == guild_id)
                .rev()
                .take(limit)
                .cloned()
                .collect();
            Ok(filtered)
        }
    }

    #[tokio::test]
    async fn test_award_coins() {
        let store = InMemoryCoinStore::new();
        let service = EconomyService::new(store);

        let balance = service
            .award_coins(1, 1, 50, "Test reward".to_string())
            .await
            .unwrap();

        assert_eq!(balance, 50);
    }

    #[tokio::test]
    async fn test_daily_claim() {
        let store = InMemoryCoinStore::new();
        let service = EconomyService::new(store);

        // First claim should succeed
        let result = service.claim_daily(1, 1).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().coins_awarded, 10);

        // Second claim immediately should fail (on cooldown)
        let result = service.claim_daily(1, 1).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_balance() {
        let store = InMemoryCoinStore::new();
        let service = EconomyService::new(store);

        // Initial balance should be 0
        let balance = service.get_balance(1, 1).await.unwrap();
        assert_eq!(balance, 0);

        // After awarding coins
        service
            .award_coins(1, 1, 75, "Test".to_string())
            .await
            .unwrap();
        let balance = service.get_balance(1, 1).await.unwrap();
        assert_eq!(balance, 75);
    }

    #[tokio::test]
    async fn test_transactions_logged() {
        let store = InMemoryCoinStore::new();
        let service = EconomyService::new(store);

        service
            .award_coins(1, 1, 10, "Reward 1".to_string())
            .await
            .unwrap();
        service
            .award_coins(1, 1, 20, "Reward 2".to_string())
            .await
            .unwrap();

        let transactions = service.get_recent_transactions(1, 1, 10).await.unwrap();
        assert_eq!(transactions.len(), 2);
        assert_eq!(transactions[0].amount, 20); // Most recent first
        assert_eq!(transactions[1].amount, 10);
    }
}
