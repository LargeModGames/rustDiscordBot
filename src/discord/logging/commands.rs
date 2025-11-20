use crate::discord::{Context, Error};
use poise::serenity_prelude as serenity;

/// Manage activity logging configuration.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    subcommands("status", "set_channel", "enable", "disable")
)]
pub async fn logging(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show current logging configuration.
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?.get();
    let config = ctx.data().logging.get_config(guild_id).await?;

    let (status, channel_mention) = if let Some(cfg) = config {
        let status = if cfg.enabled && cfg.channel_id.is_some() {
            "Enabled"
        } else {
            "Disabled"
        };
        let mention = cfg
            .channel_id
            .map(|id| format!("<#{}>", id))
            .unwrap_or_else(|| "Not set".to_string());
        (status, mention)
    } else {
        ("Disabled", "Not set".to_string())
    };

    let embed = serenity::CreateEmbed::default()
        .title("Activity Logging Configuration")
        .color(serenity::Color::BLURPLE)
        .field("Status", status, false)
        .field("Log Channel", channel_mention, false)
        .field(
            "Tracked Events",
            "• Member Join/Leave\n• Message Edit/Delete\n• Voice Activity",
            false,
        )
        .footer(serenity::CreateEmbedFooter::new(format!(
            "Guild ID: {}",
            guild_id
        )))
        .timestamp(serenity::Timestamp::now());

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Select the text channel used for logging.
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn set_channel(
    ctx: Context<'_>,
    #[description = "Channel to log to"] channel: serenity::Channel,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?.get();
    let channel_id = channel.id().get();

    ctx.data()
        .logging
        .set_log_channel(guild_id, channel_id)
        .await?;
    ctx.say(format!("✅ Logging channel set to <#{}>.", channel_id))
        .await?;
    Ok(())
}

/// Enable activity logging.
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn enable(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?.get();

    if ctx.data().logging.set_enabled(guild_id, true).await? {
        ctx.say("✅ Activity logging enabled.").await?;
    } else {
        ctx.say("Please configure a logging channel first using `/logging set_channel #channel`.")
            .await?;
    }
    Ok(())
}

/// Disable activity logging.
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn disable(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?.get();

    if ctx.data().logging.set_enabled(guild_id, false).await? {
        ctx.say("🛑 Activity logging disabled.").await?;
    } else {
        ctx.say("Logging is not configured for this server.")
            .await?;
    }
    Ok(())
}
