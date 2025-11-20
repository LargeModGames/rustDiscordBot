use super::logging_models::{LogConfig, LogEvent, TrackedMessage};
use anyhow::Result;
use async_trait::async_trait;
use dashmap::{DashMap, DashSet};

// Hardcoded meeting stage channel ID from Python code
const MEETING_STAGE_CHANNEL_ID: u64 = 1393369518297972758;
// Cap how many messages we keep in memory for logging so we don't grow unbounded.
const MAX_TRACKED_MESSAGES: usize = 5_000;

#[async_trait]
pub trait LogConfigStore: Send + Sync {
    async fn get_config(&self, guild_id: u64) -> Result<Option<LogConfig>>;
    #[allow(dead_code)]
    async fn save_config(&self, config: LogConfig) -> Result<()>;
}

pub struct LoggingService<S: LogConfigStore> {
    store: S,
    // Guild ID -> Set of Active Channel IDs
    active_voice_channels: DashMap<u64, DashSet<u64>>,
    // Channel ID -> Set of User IDs (attendees)
    meeting_attendees: DashMap<u64, DashSet<u64>>,
    // Message ID -> Snapshot for logging edits/deletes even if Serenity's cache evicts them
    message_cache: DashMap<u64, TrackedMessage>,
}

pub struct VoiceUpdateParams {
    pub guild_id: u64,
    pub member_id: u64,
    pub old_channel_id: Option<u64>,
    pub new_channel_id: Option<u64>,
    pub old_channel_members: Vec<u64>,
    pub new_channel_members: Vec<u64>,
}

impl<S: LogConfigStore> LoggingService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            active_voice_channels: DashMap::new(),
            meeting_attendees: DashMap::new(),
            message_cache: DashMap::new(),
        }
    }

    pub async fn get_config(&self, guild_id: u64) -> Result<Option<LogConfig>> {
        self.store.get_config(guild_id).await
    }

    #[allow(dead_code)]
    pub async fn set_log_channel(&self, guild_id: u64, channel_id: u64) -> Result<()> {
        let config = LogConfig {
            guild_id,
            enabled: true,
            channel_id: Some(channel_id),
        };
        self.store.save_config(config).await
    }

    #[allow(dead_code)]
    pub async fn set_enabled(&self, guild_id: u64, enabled: bool) -> Result<bool> {
        if let Some(mut config) = self.store.get_config(guild_id).await? {
            if enabled && config.channel_id.is_none() {
                return Ok(false);
            }
            config.enabled = enabled;
            self.store.save_config(config).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Store a message snapshot so we can later log deletes/edits reliably.
    pub fn remember_message(&self, message: TrackedMessage) {
        self.message_cache.insert(message.message_id, message);

        // Simple eviction: drop an arbitrary entry once we cross the cap.
        if self.message_cache.len() > MAX_TRACKED_MESSAGES {
            if let Some(first_key) = self.message_cache.iter().next().map(|entry| *entry.key()) {
                self.message_cache.remove(&first_key);
            }
        }
    }

    /// Get a tracked message without removing it (used for edits).
    pub fn get_tracked_message(&self, message_id: u64) -> Option<TrackedMessage> {
        self.message_cache.get(&message_id).map(|m| m.clone())
    }

    /// Remove a tracked message (used for deletions).
    pub fn take_tracked_message(&self, message_id: u64) -> Option<TrackedMessage> {
        self.message_cache.remove(&message_id).map(|(_, msg)| msg)
    }

    pub async fn process_voice_update(
        &self,
        params: VoiceUpdateParams,
        get_member_mention: impl Fn(u64) -> String, // Callback to get mentions
    ) -> Result<Vec<LogEvent>> {
        let guild_id = params.guild_id;
        let member_id = params.member_id;
        let old_channel_id = params.old_channel_id;
        let new_channel_id = params.new_channel_id;
        let old_channel_members = params.old_channel_members;
        let new_channel_members = params.new_channel_members;

        let mut events = Vec::new();

        let config = self.store.get_config(guild_id).await?;
        if config.is_none() || !config.as_ref().unwrap().enabled {
            return Ok(events);
        }

        // 1. Meeting Stage Logic
        if let Some(new_id) = new_channel_id {
            if new_id == MEETING_STAGE_CHANNEL_ID {
                // Joined meeting
                self.meeting_attendees
                    .entry(new_id)
                    .or_insert_with(DashSet::new)
                    .insert(member_id);
            }
        }

        if let Some(old_id) = old_channel_id {
            if old_id == MEETING_STAGE_CHANNEL_ID {
                // Left meeting
                if old_channel_members.is_empty() {
                    if let Some((_, attendees)) = self.meeting_attendees.remove(&old_id) {
                        let total = attendees.len();
                        let attendee_list: Vec<String> =
                            attendees.iter().map(|id| get_member_mention(*id)).collect();

                        events.push(LogEvent::MeetingEnded {
                            guild_id,
                            channel_id: old_id,
                            total_attendees: total,
                            attendees: attendee_list,
                        });
                    }
                }
            }
        }

        // 2. Regular Voice Channel Logic
        // Check old channel (can only go Active -> Inactive on leave)
        if let Some(old_id) = old_channel_id {
            if old_id != MEETING_STAGE_CHANNEL_ID {
                let was_active = self.is_channel_active(guild_id, old_id);
                let is_active = old_channel_members.len() >= 2;

                if !is_active && was_active {
                    self.mark_channel_active(guild_id, old_id, false);
                    events.push(LogEvent::VoiceChannelInactive {
                        guild_id,
                        channel_id: old_id,
                        member_count: old_channel_members.len(),
                    });
                }
            }
        }

        // Check new channel (can only go Inactive -> Active on join)
        if let Some(new_id) = new_channel_id {
            if new_id != MEETING_STAGE_CHANNEL_ID {
                let was_active = self.is_channel_active(guild_id, new_id);
                let is_active = new_channel_members.len() >= 2;

                if is_active && !was_active {
                    self.mark_channel_active(guild_id, new_id, true);
                    let mentions = new_channel_members
                        .iter()
                        .map(|id| get_member_mention(*id))
                        .collect();
                    events.push(LogEvent::VoiceChannelActive {
                        guild_id,
                        channel_id: new_id,
                        member_count: new_channel_members.len(),
                        members: mentions,
                    });
                }
            }
        }

        Ok(events)
    }

    fn is_channel_active(&self, guild_id: u64, channel_id: u64) -> bool {
        if let Some(guild_channels) = self.active_voice_channels.get(&guild_id) {
            guild_channels.contains(&channel_id)
        } else {
            false
        }
    }

    fn mark_channel_active(&self, guild_id: u64, channel_id: u64, active: bool) {
        let guild_channels = self
            .active_voice_channels
            .entry(guild_id)
            .or_insert_with(DashSet::new);
        if active {
            guild_channels.insert(channel_id);
        } else {
            guild_channels.remove(&channel_id);
        }
    }
}
