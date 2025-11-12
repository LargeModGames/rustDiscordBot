// Discord commands for the leveling system.
//
// **Notice the pattern:**
// 1. Extract primitive data from Discord types
// 2. Call core service
// 3. Format the response based on the result
//
// This layer is THIN - no business logic, just translation.

use crate::core::leveling::{Difficulty, LevelingService, XpSource};
use crate::infra::leveling::InMemoryXpStore;
use poise::serenity_prelude as serenity;
use std::time::Duration;

/// Type alias for our bot's context.
/// This is what every command receives as its first parameter.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Data that's shared across all commands.
/// This is where we store our services and configuration.
pub struct Data {
    pub leveling: LevelingService<InMemoryXpStore>,
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

/// Show your current level and XP.
///
/// **Command syntax:** `/level` or `/level @user`
///
/// The `#[poise::command(...)]` macro registers this as a slash command.
/// - `slash_command` = register as a Discord slash command
/// - `guild_only` = only works in guilds (not DMs)
#[poise::command(slash_command, guild_only)]
pub async fn level(
    ctx: Context<'_>,
    #[description = "User to check (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    // 1. Extract primitive data from Discord types
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_id = target_user.id.get(); // .get() extracts the u64 from serenity::UserId
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    // Don't show stats for bots
    if target_user.bot {
        ctx.say("Bots don't earn XP! 🤖").await?;
        return Ok(());
    }

    // 2. Call core service
    let stats = ctx
        .data()
        .leveling
        .get_user_stats(user_id, guild_id)
        .await?;
    debug_assert_eq!(stats.guild_id, guild_id, "Stats pulled from wrong guild");

    // 3. Format response using Discord features (embeds)
    let leveling = &ctx.data().leveling;
    let previous_threshold = leveling.xp_for_level(stats.level);
    let next_threshold = leveling.xp_for_next_level(stats.level);
    let xp_progress = stats.xp.saturating_sub(previous_threshold);
    let level_span = next_threshold.saturating_sub(previous_threshold);
    let xp_needed = next_threshold.saturating_sub(stats.xp);
    let last_activity = stats
        .last_xp_gain
        .as_ref()
        .map(|instant| format_duration(instant.elapsed()))
        .unwrap_or_else(|| "No activity yet".to_string());

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .title(format!("📊 Level Stats for {}", target_user.name))
                .color(0x00ff00) // Green color
                .field("Level", format!("**{}**", stats.level), true)
                .field("Total XP", format!("**{}**", stats.xp), true)
                .field(
                    "XP Progress",
                    format!("{} / {} ({} to go)", xp_progress, level_span, xp_needed),
                    false,
                )
                .thumbnail(target_user.face()) // User's avatar
                .field("Last Activity", last_activity, true)
                .footer(serenity::CreateEmbedFooter::new(
                    "Keep chatting to earn more XP!",
                )),
        ),
    )
    .await?;

    Ok(())
}

/// Show the server's XP leaderboard.
///
/// **Command syntax:** `/leaderboard` or `/leaderboard 2` (for page 2)
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

    // 2. Call core service
    let leaderboard = ctx
        .data()
        .leveling
        .get_leaderboard(guild_id, per_page)
        .await?;

    // Check if we have any data
    if leaderboard.is_empty() {
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
    for (index, stats) in leaderboard.iter().enumerate() {
        let rank = offset + index + 1;
        debug_assert_eq!(
            stats.guild_id, guild_id,
            "Leaderboard returned stats for the wrong guild"
        );

        // Try to get the username from cache, fall back to ID if not found
        let user_name = ctx
            .serenity_context()
            .cache
            .user(stats.user_id)
            .map(|u| u.name.clone())
            .unwrap_or_else(|| format!("User {}", stats.user_id));

        // Add medal emojis for top 3
        let medal = match rank {
            1 => "🥇",
            2 => "🥈",
            3 => "🥉",
            _ => "  ",
        };

        description.push_str(&format!(
            "{} **#{}** {} - Level {} ({} XP)\n",
            medal, rank, user_name, stats.level, stats.xp
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
                    offset + leaderboard.len()
                ))),
        ),
    )
    .await?;

    Ok(())
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

/// Convert a duration into a short human-readable string (e.g. "2m 5s ago").
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();

    match secs {
        0..=59 => format!("{}s ago", secs),
        60..=3599 => {
            let minutes = secs / 60;
            let seconds = secs % 60;
            if seconds == 0 {
                format!("{}m ago", minutes)
            } else {
                format!("{}m {}s ago", minutes, seconds)
            }
        }
        _ => {
            let hours = secs / 3600;
            let minutes = (secs % 3600) / 60;
            if minutes == 0 {
                format!("{}h ago", hours)
            } else {
                format!("{}h {}m ago", hours, minutes)
            }
        }
    }
}
