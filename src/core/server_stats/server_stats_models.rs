use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatsConfig {
    pub guild_id: u64,
    pub category_id: u64,
    pub total_members_channel_id: u64,
    pub members_channel_id: u64,
    pub bots_channel_id: u64,
    pub boost_channel_id: u64,
    pub enabled: bool,
}
