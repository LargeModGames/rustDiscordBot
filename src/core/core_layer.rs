// The core module contains all business logic.
// Each feature gets its own submodule.

#[path = "leveling/leveling_service.rs"]
pub mod leveling;

#[path = "server_stats/mod.rs"]
pub mod server_stats;

#[path = "timezones/timezone_service.rs"]
pub mod timezones;
