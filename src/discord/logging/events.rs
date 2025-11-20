use crate::core::logging::{LogEvent, TrackedMessage};
use crate::discord::logging::formatter::format_log_event;
use crate::discord::Data;
use anyhow::Result;
use poise::serenity_prelude::{self as serenity, Context, Mentionable};

pub async fn handle_voice_state_update(
    ctx: &Context,
    data: &Data,
    old: Option<&serenity::VoiceState>,
    new: &serenity::VoiceState,
) -> Result<()> {
    let guild_id = match new.guild_id {
        Some(id) => id.get(),
        None => return Ok(()),
    };

    let member = match new.member.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };

    if member.user.bot {
        return Ok(());
    }

    let user_id = member.user.id.get();
    let old_channel_id = old.and_then(|s| s.channel_id.map(|id| id.get()));
    let new_channel_id = new.channel_id.map(|id| id.get());

    let (old_members, new_members) = {
        let guild = match ctx.cache.guild(guild_id) {
            Some(g) => g,
            None => return Ok(()),
        };

        let get_members = |channel_id: u64| -> Vec<u64> {
            guild
                .voice_states
                .values()
                .filter(|vs| vs.channel_id.map(|id| id.get()) == Some(channel_id))
                .filter_map(|vs| {
                    let user_id = vs.user_id;
                    if let Some(member) = guild.members.get(&user_id) {
                        if member.user.bot {
                            None
                        } else {
                            Some(user_id.get())
                        }
                    } else {
                        Some(user_id.get())
                    }
                })
                .collect()
        };

        let old = old_channel_id.map(|id| get_members(id)).unwrap_or_default();
        let new = new_channel_id.map(|id| get_members(id)).unwrap_or_default();
        (old, new)
    };

    let events = data
        .logging
        .process_voice_update(
            guild_id,
            user_id,
            old_channel_id,
            new_channel_id,
            old_members,
            new_members,
            |uid| format!("<@{}>", uid),
        )
        .await?;

    for event in events {
        send_log(ctx, data, guild_id, event).await?;
    }

    Ok(())
}

pub async fn handle_member_join(
    ctx: &Context,
    data: &Data,
    member: &serenity::Member,
) -> Result<()> {
    let guild_id = member.guild_id.get();
    let user_id = member.user.id.get();

    let event = LogEvent::MemberJoined {
        guild_id,
        user_id,
        user_mention: member.mention().to_string(),
        avatar_url: member.user.avatar_url(),
        created_at: *member.user.created_at(),
    };

    send_log(ctx, data, guild_id, event).await?;
    Ok(())
}

pub async fn handle_member_remove(
    ctx: &Context,
    data: &Data,
    guild_id: serenity::GuildId,
    user: &serenity::User,
    member_data: Option<&serenity::Member>,
) -> Result<()> {
    let guild_id = guild_id.get();
    let user_id = user.id.get();

    let event = LogEvent::MemberLeft {
        guild_id,
        user_id,
        user_mention: user.mention().to_string(),
        avatar_url: user.avatar_url(),
        joined_at: member_data.and_then(|m| m.joined_at).map(|t| *t),
    };

    send_log(ctx, data, guild_id, event).await?;
    Ok(())
}

pub async fn handle_message_delete(
    ctx: &Context,
    data: &Data,
    channel_id: serenity::ChannelId,
    message_id: serenity::MessageId,
    guild_id: Option<serenity::GuildId>,
) -> Result<()> {
    let guild_id = match guild_id {
        Some(id) => id.get(),
        None => return Ok(()),
    };

    let message_id_u64 = message_id.get();

    // Prefer our own snapshot over the Serenity cache so we never miss deletes.
    let snapshot = data
        .logging
        .take_tracked_message(message_id_u64)
        .or_else(|| {
            ctx.cache
                .message(channel_id, message_id)
                .and_then(|message| {
                    if message.author.bot {
                        return None;
                    }

                    Some(TrackedMessage {
                        message_id: message.id.get(),
                        guild_id,
                        channel_id: message.channel_id.get(),
                        author_id: message.author.id.get(),
                        author_name: message.author.name.clone(),
                        content: message.content.clone(),
                        attachments: message
                            .attachments
                            .iter()
                            .map(|a| a.filename.clone())
                            .collect::<Vec<_>>(),
                        avatar_url: message.author.avatar_url(),
                    })
                })
        });

    let snapshot = match snapshot {
        Some(msg) => msg,
        None => return Ok(()),
    };

    if snapshot.guild_id != guild_id {
        return Ok(());
    }

    let event = LogEvent::MessageDeleted {
        guild_id,
        author_id: snapshot.author_id,
        author_name: snapshot.author_name,
        channel_id: snapshot.channel_id,
        content: snapshot.content,
        attachments: snapshot.attachments,
        avatar_url: snapshot.avatar_url,
    };

    send_log(ctx, data, guild_id, event).await?;
    Ok(())
}

pub async fn handle_message_update(
    ctx: &Context,
    data: &Data,
    old: Option<&serenity::Message>,
    _new: Option<&serenity::Message>,
    event: &serenity::MessageUpdateEvent,
) -> Result<()> {
    let guild_id = match event.guild_id {
        Some(id) => id.get(),
        None => return Ok(()),
    };

    let message_id = event.id.get();
    let new_content = match &event.content {
        Some(c) => c.clone(),
        None => return Ok(()),
    };

    // If we already tracked the message, use that snapshot to build the log.
    if let Some(mut tracked) = data.logging.get_tracked_message(message_id) {
        if tracked.content == new_content {
            return Ok(());
        }

        let event = LogEvent::MessageEdited {
            guild_id,
            author_id: tracked.author_id,
            author_name: tracked.author_name.clone(),
            channel_id: tracked.channel_id,
            before_content: tracked.content.clone(),
            after_content: new_content.clone(),
            avatar_url: tracked.avatar_url.clone(),
        };

        tracked.content = new_content;
        data.logging.remember_message(tracked);
        send_log(ctx, data, guild_id, event).await?;
        return Ok(());
    }

    // Fall back to the cached "old" message if we never tracked this one.
    let old_msg = match old {
        Some(m) => m,
        None => return Ok(()),
    };

    if old_msg.author.bot {
        return Ok(());
    }

    if old_msg.content == new_content {
        return Ok(());
    }

    let snapshot = TrackedMessage {
        message_id,
        guild_id,
        channel_id: event.channel_id.get(),
        author_id: old_msg.author.id.get(),
        author_name: old_msg.author.name.clone(),
        content: old_msg.content.clone(),
        attachments: old_msg
            .attachments
            .iter()
            .map(|a| a.filename.clone())
            .collect(),
        avatar_url: old_msg.author.avatar_url(),
    };

    if snapshot.guild_id != guild_id {
        return Ok(());
    }

    let event = LogEvent::MessageEdited {
        guild_id,
        author_id: snapshot.author_id,
        author_name: snapshot.author_name.clone(),
        channel_id: snapshot.channel_id,
        before_content: snapshot.content.clone(),
        after_content: new_content.clone(),
        avatar_url: snapshot.avatar_url.clone(),
    };

    let mut updated_snapshot = snapshot;
    updated_snapshot.content = new_content;
    data.logging.remember_message(updated_snapshot);
    send_log(ctx, data, guild_id, event).await?;
    Ok(())
}

async fn send_log(ctx: &Context, data: &Data, guild_id: u64, event: LogEvent) -> Result<()> {
    let config = data.logging.get_config(guild_id).await?;
    if let Some(cfg) = config {
        if cfg.enabled {
            if let Some(channel_id) = cfg.channel_id {
                let embed = format_log_event(&event);
                let channel = serenity::ChannelId::new(channel_id);

                if let Err(e) = channel
                    .send_message(&ctx.http, serenity::CreateMessage::new().embed(embed))
                    .await
                {
                    tracing::warn!("Failed to send log to channel {}: {}", channel_id, e);
                }
            }
        }
    }
    Ok(())
}
