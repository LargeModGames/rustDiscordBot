// The infra module contains implementations of core traits.
// Each feature implementation goes in its own submodule.

#[path = "leveling/leveling_store.rs"]
pub mod leveling;

#[path = "server_stats/mod.rs"]
pub mod server_stats;

#[path = "logging/mod.rs"]
pub mod logging;

#[path = "github/mod.rs"]
pub mod github;

#[path = "ai/mod.rs"]
pub mod ai;

#[path = "economy/mod.rs"]
pub mod economy;

#[path = "google_docs/mod.rs"]
pub mod google_docs;
