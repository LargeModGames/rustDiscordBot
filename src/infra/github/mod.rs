// GitHub infra layer.
// - `github_client.rs` talks to the GitHub HTTP API.
// - `file_store.rs` persists tracking config to disk.

#[path = "github_client.rs"]
pub mod github_client;

#[path = "file_store.rs"]
pub mod file_store;
