// Core moderation module - contains anti-spam business logic.
// Following the same pattern as logging module.

pub mod moderation_models;
pub mod moderation_service;

pub use moderation_models::*;
pub use moderation_service::*;
