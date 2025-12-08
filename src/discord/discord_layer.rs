// Discord layer - commands and event handlers.

#[path = "commands/command_catalog.rs"]
pub mod commands;

#[path = "leveling/leveling_announcements.rs"]
pub mod leveling_announcements;

#[path = "logging/mod.rs"]
pub mod logging;

#[path = "github/mod.rs"]
pub mod github;

#[path = "ai/mod.rs"]
pub mod ai;

#[path = "moderation/mod.rs"]
pub mod moderation;

// Re-export command types for convenience
pub use commands::leveling::Context;
pub use commands::leveling::{Data, Error};
