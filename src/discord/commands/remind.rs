// Discord command for setting reminders.
//
// Users can set a reminder with an optional time duration.
// After the time passes, the bot pings them in the same channel.

use crate::discord::commands::leveling::{Context, Error};
use poise::serenity_prelude::{self as serenity, Mentionable};

/// Set a reminder. The bot will ping you after the specified time.
///
/// **Examples:**
/// - `/remind message:"Take a break" time:"30 minutes"`
/// - `/remind message:"Check the oven" time:"1 hour"`
/// - `/remind message:"Meeting tomorrow" time:"1 day"`
#[poise::command(slash_command, guild_only)]
pub async fn remind(
    ctx: Context<'_>,
    #[description = "What to remind you about"] message: String,
    #[description = "When to remind you (e.g. '30 minutes', '2 hours', '1 day')"] time: Option<
        String,
    >,
) -> Result<(), Error> {
    let duration = match &time {
        Some(time_str) => match parse_duration(time_str) {
            Some(dur) => dur,
            None => {
                ctx.say(
                    "Invalid time format. Use formats like:\n\
                    - `30 seconds` or `30s`\n\
                    - `5 minutes` or `5m`\n\
                    - `2 hours` or `2h`\n\
                    - `1 day` or `1d`",
                )
                .await?;
                return Ok(());
            }
        },
        // Default to 1 minute if no time specified
        None => std::time::Duration::from_secs(60),
    };

    // Validate duration isn't too long (max 30 days) or too short (min 10 seconds)
    if duration.as_secs() < 10 {
        ctx.say("Reminder must be at least 10 seconds in the future.")
            .await?;
        return Ok(());
    }

    if duration.as_secs() > 30 * 24 * 60 * 60 {
        ctx.say("Reminder cannot be more than 30 days in the future.")
            .await?;
        return Ok(());
    }

    // Get info needed for the reminder
    let user_id = ctx.author().id;
    let channel_id = ctx.channel_id();
    let http = ctx.serenity_context().http.clone();
    let reminder_message = message.clone();

    // Format the duration for display
    let time_display = format_duration(duration);

    // Spawn a background task to send the reminder
    tokio::spawn(async move {
        tokio::time::sleep(duration).await;

        // Send the reminder ping
        let content = format!("{} Reminder: {}", user_id.mention(), reminder_message);

        if let Err(e) = channel_id
            .send_message(
                &http,
                serenity::CreateMessage::new()
                    .content(content)
                    .allowed_mentions(
                        serenity::CreateAllowedMentions::new().users(vec![user_id]),
                    ),
            )
            .await
        {
            tracing::error!("Failed to send reminder: {}", e);
        }
    });

    // Confirm the reminder was set
    ctx.say(format!(
        "Reminder set! I'll ping you in {} with: \"{}\"",
        time_display, message
    ))
    .await?;

    Ok(())
}

/// Parse a duration string like "30 minutes", "2h", "1 day" into a Duration.
fn parse_duration(input: &str) -> Option<std::time::Duration> {
    let input = input.trim().to_lowercase();

    // Try to parse formats like "30m", "2h", "1d", "45s"
    if let Some(duration) = parse_compact_format(&input) {
        return Some(duration);
    }

    // Try to parse formats like "30 minutes", "2 hours", "1 day"
    if let Some(duration) = parse_verbose_format(&input) {
        return Some(duration);
    }

    None
}

/// Parse compact formats like "30m", "2h", "1d", "45s"
fn parse_compact_format(input: &str) -> Option<std::time::Duration> {
    let input = input.trim();

    // Check for unit suffix
    let (num_str, multiplier) = if input.ends_with('s') && !input.ends_with("seconds") {
        // Could be just "s" for seconds or a number ending in 's'
        let num_part = input.trim_end_matches('s').trim();
        // Check if it's a number (not "minute" etc)
        if num_part.chars().all(|c| c.is_ascii_digit()) {
            (num_part, 1u64)
        } else {
            return None;
        }
    } else if input.ends_with('m') && !input.ends_with("minutes") {
        (input.trim_end_matches('m').trim(), 60)
    } else if input.ends_with('h') {
        (input.trim_end_matches('h').trim(), 3600)
    } else if input.ends_with('d') {
        (input.trim_end_matches('d').trim(), 86400)
    } else if input.ends_with('w') {
        (input.trim_end_matches('w').trim(), 604800)
    } else {
        return None;
    };

    let number: u64 = num_str.parse().ok()?;
    Some(std::time::Duration::from_secs(number * multiplier))
}

/// Parse verbose formats like "30 minutes", "2 hours", "1 day"
fn parse_verbose_format(input: &str) -> Option<std::time::Duration> {
    let parts: Vec<&str> = input.split_whitespace().collect();

    if parts.len() != 2 {
        return None;
    }

    let number: u64 = parts[0].parse().ok()?;
    let unit = parts[1];

    let multiplier = match unit {
        "second" | "seconds" | "sec" | "secs" => 1,
        "minute" | "minutes" | "min" | "mins" => 60,
        "hour" | "hours" | "hr" | "hrs" => 3600,
        "day" | "days" => 86400,
        "week" | "weeks" => 604800,
        _ => return None,
    };

    Some(std::time::Duration::from_secs(number * multiplier))
}

/// Format a Duration into a human-readable string
fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs < 60 {
        format!("{} second{}", total_secs, if total_secs == 1 { "" } else { "s" })
    } else if total_secs < 3600 {
        let mins = total_secs / 60;
        format!("{} minute{}", mins, if mins == 1 { "" } else { "s" })
    } else if total_secs < 86400 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        if mins > 0 {
            format!(
                "{} hour{} and {} minute{}",
                hours,
                if hours == 1 { "" } else { "s" },
                mins,
                if mins == 1 { "" } else { "s" }
            )
        } else {
            format!("{} hour{}", hours, if hours == 1 { "" } else { "s" })
        }
    } else {
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        if hours > 0 {
            format!(
                "{} day{} and {} hour{}",
                days,
                if days == 1 { "" } else { "s" },
                hours,
                if hours == 1 { "" } else { "s" }
            )
        } else {
            format!("{} day{}", days, if days == 1 { "" } else { "s" })
        }
    }
}
