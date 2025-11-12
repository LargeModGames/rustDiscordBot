// Discord layer - commands and event handlers.

#[path = "commands/command_catalog.rs"]
pub mod commands;

// Re-export command types for convenience
pub use commands::leveling::{Data, Error};
