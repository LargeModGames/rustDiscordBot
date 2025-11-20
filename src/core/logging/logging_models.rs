use chrono::{DateTime, Utc};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub guild_id: u64,
    pub enabled: bool,
    pub channel_id: Option<u64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum LogEvent {
    VoiceChannelActive {
        guild_id: u64,
        channel_id: u64,
        member_count: usize,
        members: Vec<String>, // Mentions
    },
    VoiceChannelInactive {
        guild_id: u64,
        channel_id: u64,
        member_count: usize,
    },
    MeetingEnded {
        guild_id: u64,
        channel_id: u64,
        total_attendees: usize,
        attendees: Vec<String>, // Mentions
    },
    MemberJoined {
        guild_id: u64,
        user_id: u64,
        user_mention: String,
        avatar_url: Option<String>,
        created_at: DateTime<Utc>,
    },
    MemberLeft {
        guild_id: u64,
        user_id: u64,
        user_mention: String,
        avatar_url: Option<String>,
        joined_at: Option<DateTime<Utc>>,
    },
    MessageDeleted {
        guild_id: u64,
        author_id: u64,
        author_name: String,
        channel_id: u64,
        content: String,
        attachments: Vec<String>,
        avatar_url: Option<String>,
    },
    MessageEdited {
        guild_id: u64,
        author_id: u64,
        author_name: String,
        channel_id: u64,
        before_content: String,
        after_content: String,
        avatar_url: Option<String>,
    },
}

/// Minimal snapshot of a message that we keep in-memory so
/// deletions/edits can be logged even if Serenity's cache
/// has already evicted the original message.
#[derive(Debug, Clone)]
pub struct TrackedMessage {
    pub message_id: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub author_id: u64,
    pub author_name: String,
    pub content: String,
    pub attachments: Vec<String>,
    pub avatar_url: Option<String>,
}
