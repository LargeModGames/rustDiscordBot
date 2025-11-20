use crate::core::leveling::{DailyGoal, LevelingError, UserProfile, UserStats, XpEvent, XpStore};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Row, Sqlite};
use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;

pub struct SqliteXpStore {
    pool: Pool<Sqlite>,
}

impl SqliteXpStore {
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        // Ensure the file exists if it's a file path
        let path_str = database_url.trim_start_matches("sqlite://");
        if !database_url.contains(":memory:") && !Path::new(path_str).exists() {
            if let Some(parent) = Path::new(path_str).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(path_str)?;
        }

        let conn_str = if database_url.starts_with("sqlite:") {
            database_url.to_string()
        } else {
            format!("sqlite://{}", database_url)
        };

        let pool = SqlitePoolOptions::new().connect(&conn_str).await?;

        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_profiles (
                user_id INTEGER NOT NULL,
                guild_id INTEGER NOT NULL,
                level INTEGER NOT NULL DEFAULT 1,
                total_xp INTEGER NOT NULL DEFAULT 0,
                xp_to_next_level INTEGER NOT NULL DEFAULT 100,
                total_commands_used INTEGER NOT NULL DEFAULT 0,
                total_messages INTEGER NOT NULL DEFAULT 0,
                last_daily TEXT,
                daily_streak INTEGER NOT NULL DEFAULT 0,
                last_message_timestamp TEXT,
                achievements TEXT NOT NULL DEFAULT '[]',
                best_rank INTEGER NOT NULL DEFAULT 999,
                previous_rank INTEGER NOT NULL DEFAULT 999,
                rank_improvement INTEGER NOT NULL DEFAULT 0,
                images_shared INTEGER NOT NULL DEFAULT 0,
                long_messages INTEGER NOT NULL DEFAULT 0,
                links_shared INTEGER NOT NULL DEFAULT 0,
                goals_completed INTEGER NOT NULL DEFAULT 0,
                boost_days INTEGER NOT NULL DEFAULT 0,
                first_boost_date TEXT,
                xp_history TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (user_id, guild_id)
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS daily_goals (
                guild_id INTEGER PRIMARY KEY,
                date TEXT NOT NULL,
                target INTEGER NOT NULL,
                progress INTEGER NOT NULL,
                claimers TEXT NOT NULL DEFAULT '[]',
                completed BOOLEAN NOT NULL DEFAULT 0,
                bonus_awarded_to TEXT NOT NULL DEFAULT '[]'
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl XpStore for SqliteXpStore {
    async fn get_xp(&self, user_id: u64, guild_id: u64) -> Result<u64, LevelingError> {
        let result =
            sqlx::query("SELECT total_xp FROM user_profiles WHERE user_id = ? AND guild_id = ?")
                .bind(user_id as i64)
                .bind(guild_id as i64)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        Ok(result.map(|row| row.get::<i64, _>(0) as u64).unwrap_or(0))
    }

    async fn add_xp(&self, user_id: u64, guild_id: u64, amount: u64) -> Result<(), LevelingError> {
        // This is a bit complex because we need to initialize the row if it doesn't exist.
        // However, usually we load the profile first.
        // For a pure add_xp, we can do an UPSERT.
        // But we also need to update level, etc. which this method doesn't do.
        // The trait definition implies just adding to the counter.
        // We'll assume the profile exists or create a default one.

        // Note: This implementation is simplistic and doesn't handle level up logic.
        // The service layer usually handles the logic.

        sqlx::query(
            r#"
            INSERT INTO user_profiles (user_id, guild_id, total_xp)
            VALUES (?, ?, ?)
            ON CONFLICT(user_id, guild_id) DO UPDATE SET
            total_xp = total_xp + excluded.total_xp
            "#,
        )
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .bind(amount as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn get_leaderboard(
        &self,
        guild_id: u64,
        limit: usize,
    ) -> Result<Vec<UserStats>, LevelingError> {
        let rows = sqlx::query(
            "SELECT user_id, guild_id, total_xp, level FROM user_profiles WHERE guild_id = ? ORDER BY total_xp DESC LIMIT ?"
        )
        .bind(guild_id as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        let stats = rows
            .iter()
            .map(|row| {
                UserStats {
                    user_id: row.get::<i64, _>("user_id") as u64,
                    guild_id: row.get::<i64, _>("guild_id") as u64,
                    xp: row.get::<i64, _>("total_xp") as u64,
                    level: row.get::<i64, _>("level") as u32,
                    last_xp_gain: None, // Not stored in DB as Instant
                }
            })
            .collect();

        Ok(stats)
    }

    async fn update_last_xp_time(
        &self,
        user_id: u64,
        guild_id: u64,
        _time: Instant,
    ) -> Result<(), LevelingError> {
        // We store Utc::now() instead of Instant
        let now = Utc::now();
        sqlx::query(
            "UPDATE user_profiles SET last_message_timestamp = ? WHERE user_id = ? AND guild_id = ?"
        )
        .bind(now)
        .bind(user_id as i64)
        .bind(guild_id as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| LevelingError::StorageError(e.to_string()))?;
        Ok(())
    }

    async fn get_last_xp_time(
        &self,
        _user_id: u64,
        _guild_id: u64,
    ) -> Result<Option<Instant>, LevelingError> {
        // We can't reconstruct an Instant from DB timestamp easily across restarts.
        // Returning None forces the service to rely on other checks or just accept it.
        // The service uses this for cooldowns. If we restart, cooldowns reset.
        // If we want persistent cooldowns, we'd need to check against Utc::now() in the service.
        // But the interface asks for Instant.
        // For now, we return None as the JsonStore did.
        Ok(None)
    }

    async fn get_user_profile(
        &self,
        user_id: u64,
        guild_id: u64,
    ) -> Result<Option<UserProfile>, LevelingError> {
        let row = sqlx::query("SELECT * FROM user_profiles WHERE user_id = ? AND guild_id = ?")
            .bind(user_id as i64)
            .bind(guild_id as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        if let Some(row) = row {
            Ok(Some(row_to_profile(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn save_user_profile(&self, profile: UserProfile) -> Result<(), LevelingError> {
        let achievements_json = serde_json::to_string(&profile.achievements)
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;
        let xp_history_json = serde_json::to_string(&profile.xp_history)
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO user_profiles (
                user_id, guild_id, level, total_xp, xp_to_next_level,
                total_commands_used, total_messages, last_daily, daily_streak,
                last_message_timestamp, achievements, best_rank, previous_rank,
                rank_improvement, images_shared, long_messages, links_shared,
                goals_completed, boost_days, first_boost_date, xp_history
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, guild_id) DO UPDATE SET
                level = excluded.level,
                total_xp = excluded.total_xp,
                xp_to_next_level = excluded.xp_to_next_level,
                total_commands_used = excluded.total_commands_used,
                total_messages = excluded.total_messages,
                last_daily = excluded.last_daily,
                daily_streak = excluded.daily_streak,
                last_message_timestamp = excluded.last_message_timestamp,
                achievements = excluded.achievements,
                best_rank = excluded.best_rank,
                previous_rank = excluded.previous_rank,
                rank_improvement = excluded.rank_improvement,
                images_shared = excluded.images_shared,
                long_messages = excluded.long_messages,
                links_shared = excluded.links_shared,
                goals_completed = excluded.goals_completed,
                boost_days = excluded.boost_days,
                first_boost_date = excluded.first_boost_date,
                xp_history = excluded.xp_history
            "#,
        )
        .bind(profile.user_id as i64)
        .bind(profile.guild_id as i64)
        .bind(profile.level as i64)
        .bind(profile.total_xp as i64)
        .bind(profile.xp_to_next_level as i64)
        .bind(profile.total_commands_used as i64)
        .bind(profile.total_messages as i64)
        .bind(profile.last_daily)
        .bind(profile.daily_streak as i64)
        .bind(profile.last_message_timestamp)
        .bind(achievements_json)
        .bind(profile.best_rank as i64)
        .bind(profile.previous_rank as i64)
        .bind(profile.rank_improvement as i64)
        .bind(profile.images_shared as i64)
        .bind(profile.long_messages as i64)
        .bind(profile.links_shared as i64)
        .bind(profile.goals_completed as i64)
        .bind(profile.boost_days as i64)
        .bind(profile.first_boost_date)
        .bind(xp_history_json)
        .execute(&self.pool)
        .await
        .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        Ok(())
    }

    async fn get_all_profiles(&self, guild_id: u64) -> Result<Vec<UserProfile>, LevelingError> {
        let rows = sqlx::query("SELECT * FROM user_profiles WHERE guild_id = ?")
            .bind(guild_id as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(row_to_profile(&row)?);
        }
        Ok(profiles)
    }

    async fn get_daily_goal(&self, guild_id: u64) -> Result<Option<DailyGoal>, LevelingError> {
        let row = sqlx::query("SELECT * FROM daily_goals WHERE guild_id = ?")
            .bind(guild_id as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        if let Some(row) = row {
            let claimers_json: String = row.get("claimers");
            let bonus_json: String = row.get("bonus_awarded_to");

            Ok(Some(DailyGoal {
                date: row.get("date"),
                target: row.get::<i64, _>("target") as u64,
                progress: row.get::<i64, _>("progress") as u64,
                claimers: serde_json::from_str(&claimers_json).unwrap_or_default(),
                completed: row.get("completed"),
                bonus_awarded_to: serde_json::from_str(&bonus_json).unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn save_daily_goal(&self, guild_id: u64, goal: DailyGoal) -> Result<(), LevelingError> {
        let claimers_json = serde_json::to_string(&goal.claimers)
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;
        let bonus_json = serde_json::to_string(&goal.bonus_awarded_to)
            .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO daily_goals (guild_id, date, target, progress, claimers, completed, bonus_awarded_to)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(guild_id) DO UPDATE SET
                date = excluded.date,
                target = excluded.target,
                progress = excluded.progress,
                claimers = excluded.claimers,
                completed = excluded.completed,
                bonus_awarded_to = excluded.bonus_awarded_to
            "#
        )
        .bind(guild_id as i64)
        .bind(goal.date)
        .bind(goal.target as i64)
        .bind(goal.progress as i64)
        .bind(claimers_json)
        .bind(goal.completed)
        .bind(bonus_json)
        .execute(&self.pool)
        .await
        .map_err(|e| LevelingError::StorageError(e.to_string()))?;

        Ok(())
    }
}

fn row_to_profile(row: &sqlx::sqlite::SqliteRow) -> Result<UserProfile, LevelingError> {
    let achievements_json: String = row.get("achievements");
    let xp_history_json: String = row.get("xp_history");

    Ok(UserProfile {
        user_id: row.get::<i64, _>("user_id") as u64,
        guild_id: row.get::<i64, _>("guild_id") as u64,
        level: row.get::<i64, _>("level") as u32,
        total_xp: row.get::<i64, _>("total_xp") as u64,
        xp_to_next_level: row.get::<i64, _>("xp_to_next_level") as u64,
        total_commands_used: row.get::<i64, _>("total_commands_used") as u64,
        total_messages: row.get::<i64, _>("total_messages") as u64,
        last_daily: row.get("last_daily"),
        daily_streak: row.get::<i64, _>("daily_streak") as u32,
        last_message_timestamp: row.get("last_message_timestamp"),
        achievements: serde_json::from_str(&achievements_json).unwrap_or_default(),
        best_rank: row.get::<i64, _>("best_rank") as u32,
        previous_rank: row.get::<i64, _>("previous_rank") as u32,
        rank_improvement: row.get::<i64, _>("rank_improvement") as u32,
        images_shared: row.get::<i64, _>("images_shared") as u64,
        long_messages: row.get::<i64, _>("long_messages") as u64,
        links_shared: row.get::<i64, _>("links_shared") as u64,
        goals_completed: row.get::<i64, _>("goals_completed") as u64,
        boost_days: row.get::<i64, _>("boost_days") as u64,
        first_boost_date: row.get("first_boost_date"),
        xp_history: serde_json::from_str(&xp_history_json).unwrap_or_default(),
    })
}
