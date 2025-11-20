use crate::core::logging::LogEvent;
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateEmbedFooter};

pub fn format_log_event(event: &LogEvent) -> CreateEmbed {
    match event {
        LogEvent::VoiceChannelActive {
            guild_id,
            channel_id,
            member_count,
            members,
        } => CreateEmbed::default()
            .title("Voice Channel Active")
            .description(format!(
                "<#{}> now has {} people (conversation started)",
                channel_id, member_count
            ))
            .color(serenity::Color::from_rgb(0, 255, 0)) // Green
            .field("Members", members.join(", "), false)
            .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
            .timestamp(serenity::Timestamp::now()),

        LogEvent::VoiceChannelInactive {
            guild_id,
            channel_id,
            member_count,
        } => CreateEmbed::default()
            .title("Voice Channel Inactive")
            .description(format!(
                "<#{}> now has {} people (conversation ended)",
                channel_id, member_count
            ))
            .color(serenity::Color::from_rgb(255, 165, 0)) // Orange
            .field("Previous State", "Active conversation", false)
            .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
            .timestamp(serenity::Timestamp::now()),

        LogEvent::MeetingEnded {
            guild_id,
            channel_id,
            total_attendees,
            attendees,
        } => {
            let attendee_list = if attendees.is_empty() {
                "No attendees recorded".to_string()
            } else {
                attendees.join("\n")
            };

            CreateEmbed::default()
                .title("📊 Meeting Ended - Attendance Summary")
                .description(format!(
                    "The leadership meeting in <#{}> has concluded.",
                    channel_id
                ))
                .color(serenity::Color::BLUE)
                .field("Total Attendees", total_attendees.to_string(), false)
                .field("Attendees", attendee_list, false)
                .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
                .timestamp(serenity::Timestamp::now())
        }

        LogEvent::MemberJoined {
            guild_id,
            user_mention,
            avatar_url,
            created_at,
            ..
        } => {
            let mut embed = CreateEmbed::default()
                .title("Member Joined Server")
                .description(format!("{} has joined the server.", user_mention))
                .color(serenity::Color::from_rgb(0, 255, 0)) // Green
                .field(
                    "Account Created",
                    format!("<t:{}:R>", created_at.timestamp()),
                    false,
                )
                .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
                .timestamp(serenity::Timestamp::now());

            if let Some(url) = avatar_url {
                embed = embed.thumbnail(url);
            }
            embed
        }

        LogEvent::MemberLeft {
            guild_id,
            user_mention,
            avatar_url,
            joined_at,
            ..
        } => {
            let joined_str = if let Some(joined) = joined_at {
                format!("<t:{}:R>", joined.timestamp())
            } else {
                "Unknown".to_string()
            };

            let mut embed = CreateEmbed::default()
                .title("Member Left Server")
                .description(format!("{} has left the server.", user_mention))
                .color(serenity::Color::RED)
                .field("Joined Server", joined_str, false)
                .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
                .timestamp(serenity::Timestamp::now());

            if let Some(url) = avatar_url {
                embed = embed.thumbnail(url);
            }
            embed
        }

        LogEvent::MessageDeleted {
            guild_id,
            author_id,
            author_name,
            channel_id,
            content,
            attachments,
            avatar_url,
        } => {
            let content_display = if content.is_empty() {
                "*No content*"
            } else {
                // Truncate to 4096 chars
                if content.len() > 4096 {
                    &content[..4096]
                } else {
                    content
                }
            };

            let mut embed = CreateEmbed::default()
                .title("Message Deleted")
                .description(content_display)
                .color(serenity::Color::from_rgb(255, 165, 0)) // Orange
                .field(
                    "Author",
                    format!("{} (`{}`)", author_name, author_id),
                    false,
                )
                .field("Channel", format!("<#{}>", channel_id), false)
                .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
                .timestamp(serenity::Timestamp::now());

            if !attachments.is_empty() {
                embed = embed.field("Attachments", attachments.join("\n"), false);
            }

            if let Some(url) = avatar_url {
                embed = embed.thumbnail(url);
            }
            embed
        }

        LogEvent::MessageEdited {
            guild_id,
            author_id,
            author_name,
            channel_id,
            before_content,
            after_content,
            avatar_url,
        } => {
            let before_display = if before_content.is_empty() {
                "*No content*"
            } else {
                if before_content.len() > 1024 {
                    &before_content[..1024]
                } else {
                    before_content
                }
            };

            let after_display = if after_content.is_empty() {
                "*No content*"
            } else {
                if after_content.len() > 1024 {
                    &after_content[..1024]
                } else {
                    after_content
                }
            };

            let mut embed = CreateEmbed::default()
                .title("Message Edited")
                .description(format!("Message edited in <#{}>", channel_id))
                .color(serenity::Color::BLURPLE)
                .field(
                    "Author",
                    format!("{} (`{}`)", author_name, author_id),
                    false,
                )
                .field("Channel", format!("<#{}>", channel_id), false)
                .field("Before", before_display, false)
                .field("After", after_display, false)
                .footer(CreateEmbedFooter::new(format!("Guild ID: {}", guild_id)))
                .timestamp(serenity::Timestamp::now());

            if let Some(url) = avatar_url {
                embed = embed.thumbnail(url);
            }
            embed
        }
    }
}
