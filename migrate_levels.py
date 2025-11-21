import json
import sqlite3
import os
import sys


def migrate_levels(json_path, db_path):
    if not os.path.exists(json_path):
        print(f"Error: {json_path} not found.")
        return

    print(f"Loading levels from {json_path}...")
    with open(json_path, 'r', encoding='utf-8') as f:
        data = json.load(f)

    # Extract meta data
    meta_data = data.pop("__meta__", {})
    data.pop("__legacy__", {})  # We ignore legacy data

    # Connect to SQLite
    print(f"Connecting to database {db_path}...")
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()

    # Create tables if they don't exist (schema from SqliteXpStore)
    cursor.execute("""
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
    """)

    cursor.execute("""
    CREATE TABLE IF NOT EXISTS daily_goals (
        guild_id INTEGER PRIMARY KEY,
        date TEXT NOT NULL,
        target INTEGER NOT NULL,
        progress INTEGER NOT NULL,
        claimers TEXT NOT NULL DEFAULT '[]',
        completed BOOLEAN NOT NULL DEFAULT 0,
        bonus_awarded_to TEXT NOT NULL DEFAULT '[]'
    );
    """)

    # Migrate Users
    print("Migrating users...")
    user_count = 0
    for guild_id_str, users in data.items():
        try:
            guild_id = int(guild_id_str)
        except ValueError:
            print(f"Skipping invalid guild ID: {guild_id_str}")
            continue

        if not isinstance(users, dict):
            continue

        for user_id_str, user_data in users.items():
            try:
                user_id = int(user_id_str)
            except ValueError:
                continue

            # Extract fields with defaults
            level = user_data.get("level", 1)
            total_xp = user_data.get("total_xp", 0)
            xp_to_next_level = user_data.get("xp_to_next_level", 100)
            total_commands_used = user_data.get("total_commands_used", 0)
            total_messages = user_data.get("total_messages", 0)
            last_daily = user_data.get("last_daily")
            daily_streak = user_data.get("daily_streak", 0)
            last_message_timestamp = user_data.get("last_message_timestamp")

            achievements = json.dumps(user_data.get("achievements", []))

            best_rank = user_data.get("best_rank", 999)
            previous_rank = user_data.get("previous_rank", 999)
            rank_improvement = user_data.get("rank_improvement", 0)
            images_shared = user_data.get("images_shared", 0)
            long_messages = user_data.get("long_messages", 0)
            links_shared = user_data.get("links_shared", 0)
            goals_completed = user_data.get("goals_completed", 0)
            boost_days = user_data.get("boost_days", 0)
            first_boost_date = user_data.get("first_boost_date")

            xp_history = json.dumps(user_data.get("xp_history", []))

            cursor.execute("""
            INSERT OR REPLACE INTO user_profiles (
                user_id, guild_id, level, total_xp, xp_to_next_level,
                total_commands_used, total_messages, last_daily, daily_streak,
                last_message_timestamp, achievements, best_rank, previous_rank,
                rank_improvement, images_shared, long_messages, links_shared,
                goals_completed, boost_days, first_boost_date, xp_history
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """, (
                user_id, guild_id, level, total_xp, xp_to_next_level,
                total_commands_used, total_messages, last_daily, daily_streak,
                last_message_timestamp, achievements, best_rank, previous_rank,
                rank_improvement, images_shared, long_messages, links_shared,
                goals_completed, boost_days, first_boost_date, xp_history
            ))
            user_count += 1

    print(f"Migrated {user_count} user profiles.")

    # Migrate Daily Goals
    print("Migrating daily goals...")
    goal_count = 0
    for guild_id_str, guild_meta in meta_data.items():
        try:
            guild_id = int(guild_id_str)
        except ValueError:
            continue

        daily_goal = guild_meta.get("daily_goal")
        if daily_goal:
            date = daily_goal.get("date", "")
            target = daily_goal.get("target", 0)
            progress = daily_goal.get("progress", 0)
            claimers = json.dumps(daily_goal.get("claimers", []))
            completed = daily_goal.get("completed", False)
            bonus_awarded_to = json.dumps(
                daily_goal.get("bonus_awarded_to", []))

            cursor.execute("""
            INSERT OR REPLACE INTO daily_goals (
                guild_id, date, target, progress, claimers, completed, bonus_awarded_to
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            """, (
                guild_id, date, target, progress, claimers, completed, bonus_awarded_to
            ))
            goal_count += 1

    print(f"Migrated {goal_count} daily goals.")

    conn.commit()
    conn.close()
    print("Migration complete.")


if __name__ == "__main__":
    json_file = "levels.json"
    db_file = "data/leveling.db"

    if len(sys.argv) > 1:
        json_file = sys.argv[1]

    migrate_levels(json_file, db_file)
