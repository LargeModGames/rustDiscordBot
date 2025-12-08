// Anti-spam slash commands for configuration.

use crate::discord::{Data, Error};
use poise::serenity_prelude as serenity;

type Context<'a> = poise::Context<'a, Data, Error>;

/// Anti-spam configuration commands.
///
/// Configure anti-spam settings for your server.
#[poise::command(
    slash_command,
    subcommands("status", "enable", "disable", "config", "clear_warnings"),
    required_permissions = "MANAGE_MESSAGES",
    guild_only
)]
pub async fn antispam(_ctx: Context<'_>) -> Result<(), Error> {
    // Parent command - shows help
    Ok(())
}

/// Show current anti-spam status and settings.
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;

    let config = ctx
        .data()
        .anti_spam
        .get_config(guild_id.get())
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let status_emoji = if config.enabled { "✅" } else { "❌" };

    let embed = serenity::CreateEmbed::new()
        .title("🛡️ Anti-Spam Status")
        .color(if config.enabled { 0x00FF00 } else { 0xFF0000 })
        .field(
            "Status",
            format!(
                "{} {}",
                status_emoji,
                if config.enabled {
                    "Enabled"
                } else {
                    "Disabled"
                }
            ),
            false,
        )
        .field(
            "Rate Limit",
            format!(
                "{} messages / {} seconds\nBlock duration: {} seconds",
                config.max_messages_per_window,
                config.rate_limit_window_secs,
                config.rate_limit_block_secs
            ),
            true,
        )
        .field(
            "Duplicate Detection",
            format!("{} identical messages", config.max_duplicate_messages),
            true,
        )
        .field(
            "Mention Limit",
            format!("{} mentions per message", config.max_mentions_per_message),
            true,
        )
        .field(
            "Escalation",
            format!(
                "{} warnings → {} minute timeout",
                config.warnings_before_timeout,
                config.timeout_duration_secs / 60
            ),
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Enable anti-spam protection.
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn enable(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;

    ctx.data()
        .anti_spam
        .set_enabled(guild_id.get(), true)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    ctx.say("✅ Anti-spam protection has been **enabled**.")
        .await?;
    Ok(())
}

/// Disable anti-spam protection.
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn disable(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;

    ctx.data()
        .anti_spam
        .set_enabled(guild_id.get(), false)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    ctx.say("❌ Anti-spam protection has been **disabled**.")
        .await?;
    Ok(())
}

/// Configure anti-spam settings.
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn config(
    ctx: Context<'_>,
    #[description = "Max messages in rate limit window (default: 5)"] max_messages: Option<u32>,
    #[description = "Rate limit window in seconds (default: 5)"] window_secs: Option<u64>,
    #[description = "Block duration after rate limit in seconds (default: 30)"] block_secs: Option<
        u64,
    >,
    #[description = "Max duplicate messages (default: 3)"] max_duplicates: Option<u32>,
    #[description = "Max mentions per message (default: 10)"] max_mentions: Option<u32>,
    #[description = "Warnings before timeout (default: 3)"] max_warnings: Option<u32>,
    #[description = "Timeout duration in seconds (default: 300)"] timeout_secs: Option<u64>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;

    let mut current_config = ctx
        .data()
        .anti_spam
        .get_config(guild_id.get())
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    // Apply updates
    if let Some(v) = max_messages {
        current_config.max_messages_per_window = v;
    }
    if let Some(v) = window_secs {
        current_config.rate_limit_window_secs = v;
    }
    if let Some(v) = block_secs {
        current_config.rate_limit_block_secs = v;
    }
    if let Some(v) = max_duplicates {
        current_config.max_duplicate_messages = v;
    }
    if let Some(v) = max_mentions {
        current_config.max_mentions_per_message = v;
    }
    if let Some(v) = max_warnings {
        current_config.warnings_before_timeout = v;
    }
    if let Some(v) = timeout_secs {
        current_config.timeout_duration_secs = v;
    }

    ctx.data()
        .anti_spam
        .set_config(guild_id.get(), current_config.clone())
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    ctx.say(format!(
        "✅ Anti-spam configuration updated!\n\
         • Rate limit: {} msgs / {} sec (block: {} sec)\n\
         • Duplicates: {} max\n\
         • Mentions: {} max\n\
         • Warnings before timeout: {} → {} min timeout",
        current_config.max_messages_per_window,
        current_config.rate_limit_window_secs,
        current_config.rate_limit_block_secs,
        current_config.max_duplicate_messages,
        current_config.max_mentions_per_message,
        current_config.warnings_before_timeout,
        current_config.timeout_duration_secs / 60
    ))
    .await?;

    Ok(())
}

/// Clear warnings for a user.
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn clear_warnings(
    ctx: Context<'_>,
    #[description = "User to clear warnings for"] user: serenity::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;

    ctx.data()
        .anti_spam
        .clear_user_warnings(user.id.get(), guild_id.get())
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    ctx.say(format!("✅ Cleared all spam warnings for <@{}>.", user.id))
        .await?;
    Ok(())
}
