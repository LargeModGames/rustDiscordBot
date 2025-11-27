// Discord AI module
//
// This module contains Discord-specific AI helpers, such as fetching
// context from designated channels to give the AI background knowledge.

#[path = "context_channels.rs"]
pub mod context_channels;

pub use context_channels::fetch_context_channels;
