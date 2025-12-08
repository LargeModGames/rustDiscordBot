// Discord-specific spam handling - translates core spam results to Discord actions.

use crate::core::moderation::{AntiSpamService, SpamAction, SpamCheckResult, SpamStore};
use crate::discord::Error;
use poise::serenity_prelude as serenity;

/// Check a message for spam and apply appropriate actions.
///
/// Returns `true` if the message was spam and was handled.
pub async fn handle_message_for_spam<S: SpamStore>(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    anti_spam: &AntiSpamService<S>,
) -> Result<bool, Error> {
    // Skip bots
    if msg.author.bot {
        return Ok(false);
    }

    // Only check guild messages
    let guild_id = match msg.guild_id {
        Some(id) => id.get(),
        None => return Ok(false),
    };

    let user_id = msg.author.id.get();

    // Count mentions (users + roles)
    let mention_count = (msg.mentions.len() + msg.mention_roles.len()) as u32;

    // Check for spam
    let result = anti_spam
        .check_message(user_id, guild_id, &msg.content, mention_count)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    if !result.is_spam {
        return Ok(false);
    }

    // Apply action
    apply_spam_action(ctx, msg, &result).await?;

    Ok(true)
}

/// Apply the appropriate action for detected spam.
async fn apply_spam_action(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    result: &SpamCheckResult,
) -> Result<(), Error> {
    match &result.action {
        SpamAction::None => {}

        SpamAction::Warn {
            reason,
            warning_count,
        } => {
            // Send a warning message (ephemeral-style in the channel)
            let warnings_before_timeout: u32 = 3; // Could fetch from config
            let remaining = warnings_before_timeout.saturating_sub(*warning_count);

            let warning_msg = format!(
                "⚠️ <@{}> **Spam Warning** ({}/{}): {}\n\
                 You have {} warning{} remaining before timeout.",
                msg.author.id,
                warning_count,
                warnings_before_timeout,
                reason,
                remaining,
                if remaining == 1 { "" } else { "s" }
            );

            // Delete the spam message
            if let Err(e) = msg.delete(&ctx.http).await {
                tracing::warn!("Failed to delete spam message: {}", e);
            }

            // Send warning (will auto-delete after some time)
            if let Err(e) = msg.channel_id.say(&ctx.http, &warning_msg).await {
                tracing::warn!("Failed to send spam warning: {}", e);
            }
        }

        SpamAction::DeleteMessage { reason } => {
            // Just delete, no warning
            if let Err(e) = msg.delete(&ctx.http).await {
                tracing::warn!("Failed to delete spam message ({}): {}", reason, e);
            }
        }

        SpamAction::Timeout { duration, reason } => {
            // Delete the message
            if let Err(e) = msg.delete(&ctx.http).await {
                tracing::warn!("Failed to delete spam message: {}", e);
            }

            // Apply timeout to the user
            if let Some(guild_id) = msg.guild_id {
                let timeout_until = match serenity::Timestamp::from_unix_timestamp(
                    chrono::Utc::now().timestamp() + duration.as_secs() as i64,
                ) {
                    Ok(ts) => ts,
                    Err(e) => {
                        tracing::error!("Failed to create timeout timestamp: {}", e);
                        return Ok(());
                    }
                };

                if let Err(e) = guild_id
                    .edit_member(
                        &ctx.http,
                        msg.author.id,
                        serenity::EditMember::new()
                            .disable_communication_until_datetime(timeout_until),
                    )
                    .await
                {
                    tracing::error!("Failed to timeout user: {}", e);
                } else {
                    // Notify in channel
                    let timeout_msg = format!(
                        "🔇 <@{}> has been timed out for {} minutes: {}",
                        msg.author.id,
                        duration.as_secs() / 60,
                        reason
                    );
                    if let Err(e) = msg.channel_id.say(&ctx.http, &timeout_msg).await {
                        tracing::warn!("Failed to send timeout notification: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}
