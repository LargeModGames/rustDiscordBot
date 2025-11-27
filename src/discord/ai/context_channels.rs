// Discord AI Context Channels
//
// This module handles fetching background context from specific Discord channels
// to provide the AI with up-to-date information about the project. These channels
// typically contain announcements and sneak peeks that help the AI answer questions
// about what the team is building.

use crate::core::ai::AiMessage;
use poise::serenity_prelude as serenity;

/// Channel IDs that the AI should always pull context from.
/// These are fetched whenever someone asks the bot a question, giving it
/// background knowledge about recent announcements and project updates.
///
/// Currently configured channels:
/// - Public Announcements (1388293243015270472)
/// - Sneak Peaks (1408975407335608392)
const AI_CONTEXT_CHANNEL_IDS: &[u64] = &[
    1388293243015270472, // Public Announcements
    1408975407335608392, // Sneak Peaks
];

/// Fetches recent messages from the AI context channels (announcements, sneak peaks, etc.)
/// These messages provide the AI with background information about the project.
///
/// # Arguments
/// * `http` - The Discord HTTP client for making API requests
/// * `limit` - Maximum number of messages to fetch per channel
///
/// # Returns
/// A vector of `AiMessage` containing the combined context from all channels,
/// formatted with channel names and message metadata.
///
/// # Example
/// ```ignore
/// let context = fetch_context_channels(&ctx.http, 10).await;
/// // context now contains formatted messages from announcements and sneak peeks
/// ```
pub async fn fetch_context_channels(http: &serenity::Http, limit: u8) -> Vec<AiMessage> {
    let mut context_messages = Vec::new();

    for &channel_id in AI_CONTEXT_CHANNEL_IDS {
        let channel = serenity::ChannelId::new(channel_id);

        // Try to get channel name for better context
        let channel_name = match channel.to_channel(http).await {
            Ok(serenity::Channel::Guild(gc)) => gc.name.clone(),
            _ => format!("channel-{}", channel_id),
        };

        // Fetch messages from this context channel
        match channel
            .messages(http, serenity::GetMessages::new().limit(limit))
            .await
        {
            Ok(messages) => {
                // Add a context header for this channel
                context_messages.push(AiMessage {
                    role: "system".to_string(),
                    content: format!(
                        "--- Context from #{} (for background information) ---",
                        channel_name
                    ),
                });

                // Process messages oldest to newest for proper chronological order
                for msg in messages.iter().rev() {
                    // Skip bot messages in context channels to avoid echo
                    if msg.author.bot {
                        continue;
                    }

                    let content = format!(
                        "[{}] {}: {}",
                        msg.timestamp.format("%Y-%m-%d"),
                        msg.author.name,
                        msg.content
                    );

                    context_messages.push(AiMessage {
                        role: "user".to_string(),
                        content,
                    });
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch messages from context channel {}: {}",
                    channel_id,
                    e
                );
            }
        }
    }

    context_messages
}
