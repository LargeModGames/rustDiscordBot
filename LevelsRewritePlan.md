# Levels Rewrite Plan

This document outlines the plan for rewriting the leveling system from the Python bot to the new Rust bot. All functionality and commands found in the Python bot's `levels.py` file are intended to be rewritten in Rust, adhering to the new project's architecture and best practices.

## Feature Migration Plan

The functionality of the Python `levels.py` cog will be broken down and implemented in the appropriate layers of the Rust application.

### Core Leveling Logic (`src/core/leveling/leveling_service.rs`)

This service will contain the core business logic of the leveling system, independent of Discord or the database implementation.

*   **XP and Level Calculation**:
    *   `xp_threshold_for_level`: Implement the formula for calculating the XP required for each level.
    *   `sync_user_progress`: A function to recalculate and update a user's level based on their total XP.
*   **XP Boosts**:
    *   `has_xp_boost` and `apply_xp_boost`: Logic to check for and apply XP boosts for server boosters.
*   **Daily Rewards & Streaks**:
    *   Implement logic for daily reward claims, including streak tracking and bonus calculations.
*   **Server-wide Goals**:
    *   `calculate_daily_goal_target` and `get_daily_goal_state`: Logic for managing server-wide daily goals.
*   **Achievements**:
    *   `check_and_award_achievements`: The core logic for checking if a user has met the criteria for any achievements.
    *   The achievements themselves (currently a dictionary in Python) should be defined in a structured way, perhaps loaded from a configuration file or defined statically within the core layer.
*   **XP Event History**:
    *   `record_xp_event`: Logic to record XP gain events for a user's history.

### Discord Layer (`src/discord/`)

This layer will handle all interactions with Discord, translating user commands and events into calls to the `LevelingService`.

*   **Commands (`src/discord/commands/leveling.rs`)**:
    *   `!profile`: Fetch user data from the `LevelingService` and display it in an embed.
    *   `!xpstats`: Fetch XP history and statistics from the `LevelingService` and format them into an analytics embed.
    *   `!leaderboard`: Fetch the leaderboard data from the `LevelingService` and create a paginated embed view.
    *   `!daily`: Handle the daily command, calling the `LevelingService` to process the reward and returning the result to the user.
    *   `!achievements`: Fetch achievement status from the `LevelingService` and display it.
    *   `!nextachievement`: Query the `LevelingService` to find and display the user's next closest achievement.
*   **Event Handlers (`src/discord/discord_layer.rs`)**:
    *   `on_message`: An event handler that awards XP for messages, calling the `LevelingService`. It should include a cooldown mechanism similar to the Python bot.
    *   `on_command_completion`: An event handler to track command usage.
*   **Level-Up Announcements (`src/discord/leveling/leveling_announcements.rs`)**:
    *   `handle_level_up`: A function that, when a user levels up, sends a notification to the appropriate channel. This will be called from the event handlers after the `LevelingService` reports a level-up.

### Infrastructure Layer (`src/infra/leveling/`)

This layer will be responsible for persisting all leveling data.

*   **Storage Trait (`src/infra/leveling/leveling_store.rs`)**:
    *   Define a `LevelingStore` trait that outlines the necessary methods for storing and retrieving user data, such as `get_user_data`, `save_user_data`, `get_leaderboard`, etc.
*   **Storage Implementation**:
    *   The Python bot uses JSON files for storage. A similar approach can be taken in Rust using `serde_json`. A new `JsonLevelingStore` struct can be created that implements the `LevelingStore` trait. This will replace the existing `in_memory.rs` implementation, or be offered as a persistent alternative.

### Background Tasks

*   **`track_boosters`**: The daily task to track server boosters should be implemented as a background task in Rust, likely spawned in `main.rs`. This task will periodically call the `LevelingService` to update booster information.
