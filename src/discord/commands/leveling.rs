// Discord commands for the leveling system.
//
// **Notice the pattern:**
// 1. Extract primitive data from Discord types
// 2. Call core service
// 3. Format the response based on the result
//
// This layer is THIN - no business logic, just translation.

use crate::core::leveling::achievements::get_all_achievements;
use crate::core::leveling::{Difficulty, LevelingService, XpSource};
use crate::core::github::GithubService;
use crate::core::logging::LoggingService;
use crate::core::server_stats::ServerStatsService;
use crate::core::timezones::TimezoneService;
use crate::infra::github::file_store::GithubFileStore;
use crate::infra::github::github_client::GithubApiClient;
use crate::infra::leveling::SqliteXpStore;
use crate::infra::logging::sqlite_store::SqliteLogStore;
use crate::infra::server_stats::JsonServerStatsStore;
use poise::serenity_prelude as serenity;
use std::collections::HashMap;

/// Show your current level and XP.
#[poise::command(slash_command, guild_only)]
pub async fn level(
    ctx: Context<'_>,
    #[description = "User to check (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    show_profile(ctx, user).await
}

/// Display user's profile including level, XP, and stats.
#[poise::command(slash_command, guild_only)]
pub async fn profile(
    ctx: Context<'_>,
    #[description = "User to check (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    show_profile(ctx, user).await
}

/// Shared logic for level and profile commands
async fn show_profile(ctx: Context<'_>, user: Option<serenity::User>) -> Result<(), Error> {
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_id = target_user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    if target_user.bot {
        ctx.say("Bots don't have profiles! 🤖").await?;
        return Ok(());
    }

    let profile = ctx
        .data()
        .leveling
        .get_user_profile(user_id, guild_id)
        .await?;

    let leveling = &ctx.data().leveling;
    let previous_threshold = leveling.xp_for_level(profile.level);
    let next_threshold = leveling.xp_for_next_level(profile.level);
    let xp_progress = profile.total_xp.saturating_sub(previous_threshold);
    let level_span = next_threshold.saturating_sub(previous_threshold);
    let xp_needed = next_threshold.saturating_sub(profile.total_xp);

    let progress_pct = if level_span > 0 {
        xp_progress as f64 / level_span as f64
    } else {
        0.0
    };

    let embed = serenity::CreateEmbed::new()
        .title(format!("Profile of {}", target_user.name))
        .color(0x00ff00)
        .thumbnail(target_user.face())
        .field("Level", format!("**{}**", profile.level), true)
        .field("Total XP", format!("**{}**", profile.total_xp), true)
        .field(
            "Progress",
            format!(
                "{}/{} XP\n{}",
                xp_progress,
                level_span,
                build_progress_bar(progress_pct, 15)
            ),
            false,
        )
        .field("XP to next level", format!("{}", xp_needed), false)
        .field(
            "Total commands",
            format!("{}", profile.total_commands_used),
            true,
        )
        .field(
            "Total messages",
            format!("{}", profile.total_messages),
            true,
        )
        .field(
            "Daily streak",
            format!("{} days", profile.daily_streak),
            true,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show XP analytics for yourself or another member.
#[poise::command(slash_command, guild_only)]
pub async fn xpstats(
    ctx: Context<'_>,
    #[description = "User to check"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_id = target_user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let profile = ctx
        .data()
        .leveling
        .get_user_profile(user_id, guild_id)
        .await?;

    // Calculate stats from xp_history
    let now = chrono::Utc::now();
    let week_ago = now - chrono::Duration::days(7);

    let recent_events: Vec<_> = profile
        .xp_history
        .iter()
        .filter(|e| e.timestamp >= week_ago)
        .collect();

    let recent_total: u64 = recent_events.iter().map(|e| e.amount).sum();

    // Group by day
    let mut daily_totals: HashMap<String, u64> = HashMap::new();
    for event in &recent_events {
        let day = event.timestamp.format("%Y-%m-%d").to_string();
        *daily_totals.entry(day).or_default() += event.amount;
    }

    let active_days = daily_totals.len().max(1);
    let avg_per_day = recent_total as f64 / active_days as f64;

    let best_day = daily_totals
        .iter()
        .max_by_key(|(_, amount)| *amount)
        .map(|(day, amount)| format!("{} XP on {}", amount, day))
        .unwrap_or_else(|| "No activity".to_string());

    // Top sources
    let mut source_counts: HashMap<String, u64> = HashMap::new();
    for event in &recent_events {
        *source_counts.entry(event.source.clone()).or_default() += event.amount;
    }
    let mut sources: Vec<_> = source_counts.into_iter().collect();
    sources.sort_by(|a, b| b.1.cmp(&a.1));

    let top_sources = if sources.is_empty() {
        "No XP sources logged this week.".to_string()
    } else {
        sources
            .iter()
            .take(3)
            .map(|(source, amount)| format!("{}: {} XP", source, amount))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Recent events feed
    let recent_feed = if profile.xp_history.is_empty() {
        "No XP events recorded yet.".to_string()
    } else {
        profile
            .xp_history
            .iter()
            .rev()
            .take(5)
            .map(|e| {
                let time_diff = now - e.timestamp;
                let time_str = if time_diff.num_minutes() < 60 {
                    format!("{}m ago", time_diff.num_minutes())
                } else if time_diff.num_hours() < 24 {
                    format!("{}h ago", time_diff.num_hours())
                } else {
                    format!("{}d ago", time_diff.num_days())
                };

                let note = e
                    .note
                    .as_ref()
                    .map(|n| format!(" ({})", n))
                    .unwrap_or_default();
                format!("+{} XP — {}{} • {}", e.amount, e.source, note, time_str)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let embed = serenity::CreateEmbed::new()
        .title(format!("XP Analytics — {}", target_user.name))
        .color(0x008080) // Teal
        .thumbnail(target_user.face())
        .field("All-time XP", format!("{}", profile.total_xp), true)
        .field("Last 7 days", format!("{} XP", recent_total), true)
        .field("Avg per active day", format!("{:.1} XP", avg_per_day), true)
        .field("Top sources", top_sources, false)
        .field("Best day", best_day, false)
        .field("Recent events", recent_feed, false)
        .footer(serenity::CreateEmbedFooter::new(
            "Analytics based on last 120 events",
        ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show the server's XP leaderboard.
#[poise::command(slash_command, guild_only)]
pub async fn leaderboard(
    ctx: Context<'_>,
    #[description = "Page number (default: 1)"]
    #[min = 1]
    page: Option<usize>,
) -> Result<(), Error> {
    // 1. Extract primitive data
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();
    let page = page.unwrap_or(1);
    let per_page = 10;
    let offset = (page - 1) * per_page;

    // 2. Recalculate ranks and fetch current profiles
    let profiles = ctx
        .data()
        .leveling
        .recalculate_and_update_ranks(guild_id)
        .await?;

    // Check if we have any data
    if profiles.is_empty() {
        ctx.say("No one has earned XP yet! Start chatting to get on the leaderboard! 💬")
            .await?;
        return Ok(());
    }

    // 3. Format response
    let guild_name = ctx
        .guild()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Unknown Server".to_string());

    // Build the leaderboard text
    let mut description = String::new();
    for (index, stats) in profiles.iter().skip(offset).take(per_page).enumerate() {
        let rank = offset + index + 1;
        debug_assert_eq!(
            stats.guild_id, guild_id,
            "Leaderboard returned stats for the wrong guild"
        );

        // Resolve a display name: prefer nickname in the guild, then username, then a fetch,
        // and finally fall back to a mention so the entry is readable.
        let user_name = resolve_display_name(&ctx, guild_id, stats.user_id).await;

        // Add medal emojis for top 3
        let medal = match rank {
            1 => "🥇",
            2 => "🥈",
            3 => "🥉",
            _ => "  ",
        };

        description.push_str(&format!(
            "{} **#{}** {} - Level {} ({} XP)\n",
            medal, rank, user_name, stats.level, stats.total_xp
        ));
    }

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .title(format!("🏆 {} Leaderboard - Page {}", guild_name, page))
                .description(description)
                .color(0xffd700) // Gold color
                .footer(serenity::CreateEmbedFooter::new(format!(
                    "Showing ranks {}-{}",
                    offset + 1,
                    offset + profiles.iter().skip(offset).take(per_page).count()
                ))),
        ),
    )
    .await?;

    Ok(())
}

/// Resolve a human-friendly display name for a user.
///
/// Order of preference:
/// 1. Guild nickname (from cache)
/// 2. Guild nickname (via HTTP fetch)
/// 3. Username from cache
/// 4. Username via HTTP fetch
/// 5. Mentions as a fallback (so users can still be identified)
async fn resolve_display_name(ctx: &Context<'_>, guild_id: u64, user_id: u64) -> String {
    let guild_id_s = serenity::GuildId::from(guild_id);
    let user_id_s = serenity::UserId::from(user_id);

    // Try to get the guild member from cache first (preferred for nicknames)
    if let Some(guild) = ctx.serenity_context().cache.guild(guild_id_s) {
        if let Some(member) = guild.members.get(&user_id_s) {
            // display_name() prefers nick over username
            return member.display_name().to_string();
        }
    }

    // Try getting the user from cache
    if let Some(user) = ctx.serenity_context().cache.user(user_id_s) {
        return user.name.clone();
    }

    // As a last resort, try an HTTP fetch for the member (may fail if the user left the guild)
    if let Ok(member) = ctx
        .serenity_context()
        .http
        .get_member(guild_id_s, user_id_s)
        .await
    {
        if let Some(nick) = member.nick {
            return nick;
        }
        return member.user.name;
    }

    // Try a direct user fetch. If that succeeds, use the username.
    if let Ok(user) = ctx.serenity_context().http.get_user(user_id_s).await {
        return user.name;
    }

    // Final fallback: return a mention so it's still obvious who the entry is
    format!("<@{}>", user_id)
}

/// Type alias for our bot's context.
/// This is what every command receives as its first parameter.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Data that's shared across all commands.
/// This is where we store our services and configuration.
use std::sync::Arc;

pub struct Data {
    pub leveling: Arc<LevelingService<SqliteXpStore>>,
    pub server_stats: Arc<ServerStatsService<JsonServerStatsStore>>,
    pub timezones: Arc<TimezoneService>,
    pub logging: Arc<LoggingService<SqliteLogStore>>,
    pub github: Arc<GithubService<GithubApiClient, GithubFileStore>>,
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum AwardReason {
    #[name = "Message"]
    Message,
    #[name = "Voice Minute"]
    VoiceMinute,
    #[name = "Code Challenge"]
    CodeChallenge,
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum ChallengeDifficultyChoice {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl From<ChallengeDifficultyChoice> for Difficulty {
    fn from(value: ChallengeDifficultyChoice) -> Self {
        match value {
            ChallengeDifficultyChoice::Easy => Difficulty::Easy,
            ChallengeDifficultyChoice::Medium => Difficulty::Medium,
            ChallengeDifficultyChoice::Hard => Difficulty::Hard,
            ChallengeDifficultyChoice::Expert => Difficulty::Expert,
        }
    }
}

/// Manually award XP to a user (admin only - for testing).
///
/// **Command syntax:** `/give_xp @user 100`
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn give_xp(
    ctx: Context<'_>,
    #[description = "User to give XP to"] user: serenity::User,
    #[description = "Amount of XP to give"] amount: u64,
    #[description = "Why are you awarding XP? (default: Message)"] reason: Option<AwardReason>,
    #[description = "Challenge difficulty (when reason = Code Challenge)"]
    challenge_difficulty: Option<ChallengeDifficultyChoice>,
    #[description = "Language used for the code challenge (if applicable)"] language: Option<
        String,
    >,
    #[description = "Execution time in ms for the challenge (if applicable)"]
    execution_time_ms: Option<u64>,
) -> Result<(), Error> {
    if user.bot {
        ctx.say("You can't give XP to bots!").await?;
        return Ok(());
    }

    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let selected_reason = reason.unwrap_or(AwardReason::Message);
    let xp_source = match selected_reason {
        AwardReason::Message => XpSource::Message,
        AwardReason::VoiceMinute => XpSource::VoiceMinute,
        AwardReason::CodeChallenge => {
            let difficulty = challenge_difficulty
                .unwrap_or(ChallengeDifficultyChoice::Easy)
                .into();
            XpSource::CodeChallenge {
                difficulty,
                language: language.unwrap_or_else(|| "rust".to_string()),
                execution_time_ms: execution_time_ms.unwrap_or(0),
            }
        }
    };

    let result = ctx
        .data()
        .leveling
        .award_xp(user_id, guild_id, amount, xp_source)
        .await?;

    // Check if they leveled up
    if let Some(level_up) = result {
        ctx.say(format!(
            "✅ Gave {} XP to {} via {:?}!\n🎉 They leveled up to level {} ({} XP total)!",
            amount, user.name, selected_reason, level_up.new_level, level_up.total_xp
        ))
        .await?;
    } else {
        ctx.say(format!(
            "✅ Gave {} XP to {} via {:?}!",
            amount, user.name, selected_reason
        ))
        .await?;
    }

    Ok(())
}

/// Claim daily reward
#[poise::command(slash_command, guild_only)]
pub async fn daily_claim(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    if user.bot {
        ctx.say("Bots don't claim dailies.").await?;
        return Ok(());
    }

    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    // Detect booster status
    let boosted = ctx
        .serenity_context()
        .cache
        .guild(serenity::GuildId::from(guild_id))
        .and_then(|g| g.members.get(&serenity::UserId::from(user_id)).cloned())
        .and_then(|m| m.premium_since)
        .is_some();

    let member_count = ctx.guild().map(|g| g.member_count).unwrap_or(0);
    let (xp_award, levelup_opt) = ctx
        .data()
        .leveling
        .claim_daily(user_id, guild_id, boosted, member_count)
        .await?;

    if xp_award == 0 {
        // Show info about when they can next claim and the current server goal progress
        let profile = ctx
            .data()
            .leveling
            .get_user_profile(user_id, guild_id)
            .await?;
        let now = chrono::Utc::now();
        let next_claim = profile.last_daily.map(|d| d + chrono::Duration::days(1));
        let time_remaining = next_claim
            .map(|t| t.signed_duration_since(now))
            .unwrap_or_else(chrono::Duration::zero);
        let time_str = if time_remaining.num_seconds() <= 0 {
            "Ready soon".to_string()
        } else if time_remaining.num_minutes() < 60 {
            format!(
                "{}m {}s",
                time_remaining.num_minutes(),
                time_remaining.num_seconds() % 60
            )
        } else if time_remaining.num_hours() < 24 {
            format!(
                "{}h {}m",
                time_remaining.num_hours(),
                time_remaining.num_minutes() % 60
            )
        } else {
            format!(
                "{}d {}h",
                time_remaining.num_days(),
                time_remaining.num_hours() % 24
            )
        };

        let daily_goal = ctx
            .data()
            .leveling
            .get_daily_goal_state(guild_id, member_count)
            .await?;
        let goal_progress = daily_goal.progress as f64 / daily_goal.target as f64;
        let progress_bar =
            build_progress_bar(goal_progress, std::cmp::min(daily_goal.target as usize, 18));

        let embed = serenity::CreateEmbed::new()
            .title("Daily Reward — Already Claimed")
            .description(format!(
                "You have already claimed your daily reward. Time until next claim: {}",
                time_str
            ))
            .color(0xffa500)
            .field("Streak", format!("{} days", profile.daily_streak), true)
            .field(
                "Server Goal",
                format!(
                    "{}/{} claims\n{}",
                    daily_goal.progress, daily_goal.target, progress_bar
                ),
                false,
            );

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    // We'll show an embed with the details below
    // Show success embed with daily goal progress
    let daily_goal = ctx
        .data()
        .leveling
        .get_daily_goal_state(guild_id, member_count)
        .await?;
    let goal_progress = daily_goal.progress as f64 / daily_goal.target as f64;
    let progress_bar =
        build_progress_bar(goal_progress, std::cmp::min(daily_goal.target as usize, 18));

    let mut description = format!("You gained {} XP!", xp_award);
    if let Some(level_up) = levelup_opt {
        description = format!(
            "You gained {} XP and leveled up to {}!",
            xp_award, level_up.new_level
        );
    }

    let embed = serenity::CreateEmbed::new()
        .title("Daily Reward Claimed")
        .description(description)
        .color(0x00ff00)
        .field(
            "Streak",
            format!(
                "{} days",
                ctx.data()
                    .leveling
                    .get_user_profile(user_id, guild_id)
                    .await?
                    .daily_streak
            ),
            true,
        )
        .field(
            "Server Goal",
            format!(
                "{}/{} claims\n{}",
                daily_goal.progress, daily_goal.target, progress_bar
            ),
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show user's achievements
#[poise::command(slash_command, guild_only)]
pub async fn achievements(
    ctx: Context<'_>,
    #[description = "User to check"] member: Option<serenity::User>,
) -> Result<(), Error> {
    let target_user = member.as_ref().unwrap_or_else(|| ctx.author());
    if target_user.bot {
        ctx.say("Bots don't have achievements").await?;
        return Ok(());
    }

    let user_id = target_user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let profile = ctx
        .data()
        .leveling
        .get_user_profile(user_id, guild_id)
        .await?;

    let all_achievements = get_all_achievements();
    let earned_ids: Vec<&String> = profile.achievements.iter().collect();
    let earned_count = earned_ids.len();
    let total_count = all_achievements.len();
    let completion_pct = if total_count > 0 {
        (earned_count as f64 / total_count as f64) * 100.0
    } else {
        0.0
    };

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("🏆 {}'s Achievements", target_user.name))
        .description(format!(
            "**{}/{}** achievements unlocked ({:.1}%)\n{}",
            earned_count,
            total_count,
            completion_pct,
            build_progress_bar(completion_pct / 100.0, 15)
        ))
        .color(0xffd700) // Gold
        .thumbnail(target_user.face());

    // Group by category
    let mut by_category: HashMap<String, Vec<String>> = HashMap::new();

    // Sort achievements by category then name to ensure consistent order
    // We iterate over all defined achievements to show locked ones too
    for ach in &all_achievements {
        let is_earned = profile.achievements.contains(&ach.id);
        let status = if is_earned { "✅" } else { "🔒" };
        let emoji = if is_earned { &ach.emoji } else { "❓" };
        let name = if is_earned { &ach.name } else { "???" };
        let desc = if is_earned {
            &ach.description
        } else {
            "Locked"
        };

        let line = format!(
            "{} {} **{}**\n   _{}_ (+{} XP)",
            status, emoji, name, desc, ach.reward_xp
        );

        by_category
            .entry(ach.category.title().to_string())
            .or_default()
            .push(line);
    }

    // Add fields for each category (sorted keys)
    let mut categories: Vec<_> = by_category.keys().cloned().collect();
    categories.sort();

    for cat in categories {
        if let Some(lines) = by_category.get(&cat) {
            embed = embed.field(format!("📁 {}", cat), lines.join("\n"), false);
        }
    }

    // Calculate total XP from achievements
    let total_ach_xp: u64 = all_achievements
        .iter()
        .filter(|a| profile.achievements.contains(&a.id))
        .map(|a| a.reward_xp)
        .sum();

    embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
        "Total achievement XP earned: {}",
        total_ach_xp
    )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show the closest achievement you can earn.
#[poise::command(slash_command, guild_only, aliases("nextach"))]
pub async fn next_achievement(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let profile = ctx
        .data()
        .leveling
        .get_user_profile(user_id, guild_id)
        .await?;

    if let Some((ach, progress, current, target)) =
        ctx.data().leveling.get_next_achievement(&profile)
    {
        let embed = serenity::CreateEmbed::new()
            .title("🎯 Next Achievement")
            .description(format!(
                "{} **{}**\n_{}_",
                ach.emoji, ach.name, ach.description
            ))
            .color(0x3498db) // Blue
            .field(
                "Progress",
                format!(
                    "{}/{}\n{}",
                    current,
                    target,
                    build_progress_bar(progress, 15)
                ),
                false,
            )
            .field("Reward", format!("+{} XP", ach.reward_xp), true)
            .field("Category", ach.category.title(), true);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        ctx.say("You've unlocked all trackable achievements! 🎉")
            .await?;
    }

    Ok(())
}

fn build_progress_bar(progress: f64, length: usize) -> String {
    let clamped = progress.clamp(0.0, 1.0);
    let mut filled = (clamped * length as f64).round() as usize;
    if clamped > 0.0 && filled == 0 {
        filled = 1;
    }
    if filled > length {
        filled = length;
    }
    let filled_char = "▰";
    let empty_char = "▱";
    let bar = filled_char.repeat(filled) + &empty_char.repeat(length - filled);
    format!("{} ({}%)", bar, (clamped * 100.0).round() as u32)
}
