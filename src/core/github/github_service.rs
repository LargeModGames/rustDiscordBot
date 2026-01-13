use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

/// Errors that can be raised by the GitHub tracking workflow.
#[derive(Debug, Error)]
pub enum GithubError {
    #[error("GitHub API error: {0}")]
    Api(String),
    #[error("Failed to persist GitHub config: {0}")]
    Store(String),
}

/// Light-weight commit representation that is independent of any HTTP or Discord types.
#[derive(Debug, Clone)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub author_name: String,
    pub html_url: String,
    pub avatar_url: Option<String>,
    pub committed_at: Option<DateTime<Utc>>,
}

/// Basic issue model used for both bugs and general issue updates.
#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub reporter: Option<String>,
    pub assignee: Option<String>,
    pub closed_by: Option<String>,
    pub labels: Vec<String>,
    pub state: IssueState,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub is_bug: bool,
}

/// Whether an issue is open or closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}

/// What happened to an issue in the latest poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueActivity {
    Opened,
    Updated,
    Closed,
}

/// Event emitted by the core service. The Discord layer turns these into embeds.
#[derive(Debug, Clone)]
pub enum GithubEvent {
    CommitPushed {
        owner: String,
        repo: String,
        branch: String,
        commit: Commit,
    },
    BugClosed {
        owner: String,
        repo: String,
        issue: Issue,
    },
    IssueActivity {
        owner: String,
        repo: String,
        issue: Issue,
        activity: IssueActivity,
    },
}

/// Wrapper that includes routing information for the Discord adapter.
#[derive(Debug, Clone)]
pub struct GithubUpdate {
    pub guild_id: u64,
    pub channel_id: u64,
    pub event: GithubEvent,
}

/// Persisted state that keeps track of where we left off per repository.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoTrackingData {
    #[serde(default)]
    pub last_commit_shas: HashMap<String, String>,
    #[serde(default)]
    pub last_bug_closed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_issue_updated_at: Option<DateTime<Utc>>,
}

/// Configuration for one tracked entry (either a single repo or an org).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubTrackingEntry {
    pub owner: String,
    #[serde(default)]
    pub repo: Option<String>,
    pub channel_id: u64,
    #[serde(default)]
    pub last_commit_shas: HashMap<String, String>,
    #[serde(default)]
    pub last_bug_closed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_issue_updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_org: bool,
    #[serde(default)]
    pub org_repos: Vec<String>,
    #[serde(default)]
    pub repo_data: HashMap<String, RepoTrackingData>,
}

impl GithubTrackingEntry {
    pub fn new_repo(owner: &str, repo: &str, channel_id: u64) -> Self {
        Self {
            owner: owner.to_string(),
            repo: Some(repo.to_string()),
            channel_id,
            last_commit_shas: HashMap::new(),
            last_bug_closed_at: None,
            last_issue_updated_at: None,
            is_org: false,
            org_repos: Vec::new(),
            repo_data: HashMap::new(),
        }
    }

    pub fn new_org(org: &str, channel_id: u64, repos: Vec<String>) -> Self {
        Self {
            owner: org.to_string(),
            repo: None,
            channel_id,
            last_commit_shas: HashMap::new(),
            last_bug_closed_at: None,
            last_issue_updated_at: None,
            is_org: true,
            org_repos: repos,
            repo_data: HashMap::new(),
        }
    }
}

/// Top-level configuration map keyed by guild id.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GithubConfig {
    #[serde(default)]
    pub guilds: HashMap<u64, Vec<GithubTrackingEntry>>,
}

/// Trait describing the minimal GitHub operations needed by the service.
#[async_trait]
pub trait GithubClient: Send + Sync {
    async fn list_org_repos(&self, org: &str) -> Result<Vec<String>, GithubError>;
    async fn list_branches(&self, owner: &str, repo: &str) -> Result<Vec<String>, GithubError>;
    async fn list_commits(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        per_page: usize,
    ) -> Result<Vec<Commit>, GithubError>;
    async fn list_bug_issues(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Issue>, GithubError>;
    async fn list_general_issues(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Issue>, GithubError>;
}

/// Storage layer abstraction for GitHub configuration.
#[async_trait]
pub trait GithubConfigStore: Send + Sync {
    async fn load(&self) -> Result<GithubConfig, GithubError>;
    async fn save(&self, config: &GithubConfig) -> Result<(), GithubError>;
}

/// Service that orchestrates polling GitHub and emitting events for the Discord layer.
///
/// The polling logic lives here so it can be tested without Discord or HTTP concerns.
pub struct GithubService<C: GithubClient, S: GithubConfigStore> {
    client: C,
    store: S,
    config: RwLock<GithubConfig>,
}

impl<C, S> GithubService<C, S>
where
    C: GithubClient,
    S: GithubConfigStore,
{
    /// Create a new service and eagerly load the persisted configuration.
    pub async fn new(client: C, store: S) -> Result<Self, GithubError> {
        let config = store.load().await.unwrap_or_default();

        Ok(Self {
            client,
            store,
            config: RwLock::new(config),
        })
    }

    /// List tracked entries for a guild so the Discord layer can render them.
    pub async fn list_entries(&self, guild_id: u64) -> Vec<GithubTrackingEntry> {
        self.config
            .read()
            .await
            .guilds
            .get(&guild_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Track a single repository (all branches).
    pub async fn track_repository(
        &self,
        guild_id: u64,
        owner: &str,
        repo: &str,
        channel_id: u64,
    ) -> Result<(), GithubError> {
        let mut config = self.config.write().await;
        let entries = config.guilds.entry(guild_id).or_default();

        if let Some(existing) = entries.iter_mut().find(|e| {
            !e.is_org && e.owner.eq_ignore_ascii_case(owner) && e.repo.as_deref() == Some(repo)
        }) {
            existing.channel_id = channel_id;
        } else {
            entries.push(GithubTrackingEntry::new_repo(owner, repo, channel_id));
        }

        self.store.save(&config).await?;
        Ok(())
    }

    /// Track an organization by expanding all of its repositories.
    pub async fn track_organization(
        &self,
        guild_id: u64,
        org: &str,
        channel_id: u64,
    ) -> Result<Vec<String>, GithubError> {
        let repos = self.client.list_org_repos(org).await?;

        let mut config = self.config.write().await;
        let entries = config.guilds.entry(guild_id).or_default();

        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.is_org && e.owner.eq_ignore_ascii_case(org))
        {
            existing.channel_id = channel_id;
            existing.org_repos = repos.clone();
        } else {
            entries.push(GithubTrackingEntry::new_org(org, channel_id, repos.clone()));
        }

        self.store.save(&config).await?;
        Ok(repos)
    }

    /// Remove a single repository from tracking.
    pub async fn remove_repository(
        &self,
        guild_id: u64,
        owner: &str,
        repo: &str,
    ) -> Result<bool, GithubError> {
        let mut config = self.config.write().await;
        if let Some(entries) = config.guilds.get_mut(&guild_id) {
            let before = entries.len();
            entries.retain(|entry| {
                !(entry
                    .repo
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case(repo))
                    && entry.owner.eq_ignore_ascii_case(owner)
                    && !entry.is_org)
            });
            if entries.len() != before {
                self.store.save(&config).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Remove an organization entry.
    pub async fn remove_organization(&self, guild_id: u64, org: &str) -> Result<bool, GithubError> {
        let mut config = self.config.write().await;
        if let Some(entries) = config.guilds.get_mut(&guild_id) {
            let before = entries.len();
            entries.retain(|entry| !(entry.is_org && entry.owner.eq_ignore_ascii_case(org)));
            if entries.len() != before {
                self.store.save(&config).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Force an immediate poll and return events to be posted.
    pub async fn poll_updates(&self) -> Result<Vec<GithubUpdate>, GithubError> {
        // Clone the config so we can perform HTTP calls without holding locks.
        let snapshot = { self.config.read().await.clone() };
        let mut updates = Vec::new();

        // We collect changes to apply them safely after the poll.
        struct RepoStateUpdate {
            guild_id: u64,
            owner: String,
            repo: Option<String>, // None if org
            is_org: bool,
            // For single repo:
            new_state: Option<RepoTrackingData>,
            // For org:
            org_repos: Option<Vec<String>>,
            repo_states: Option<HashMap<String, RepoTrackingData>>,
        }

        let mut pending_changes = Vec::new();

        for (guild_id, entries) in snapshot.guilds.iter() {
            for entry in entries {
                let owner = entry.owner.clone();
                if entry.is_org {
                    let mut org_repos_update = None;
                    let mut current_repos = entry.org_repos.clone();

                    if current_repos.is_empty() {
                        let fetched = self.client.list_org_repos(&owner).await?;
                        current_repos = fetched.clone();
                        org_repos_update = Some(fetched);
                    }

                    let mut repo_states_update = HashMap::new();
                    let mut org_dirty = false;

                    for repo in current_repos {
                        let repo_key = format!("{}/{}", owner, repo);
                        let mut repo_state =
                            entry.repo_data.get(&repo_key).cloned().unwrap_or_default();

                        let (repo_updates, repo_dirty) = self
                            .poll_repository(
                                *guild_id,
                                entry.channel_id,
                                &owner,
                                &repo,
                                &mut repo_state,
                            )
                            .await?;
                        updates.extend(repo_updates);

                        if repo_dirty {
                            repo_states_update.insert(repo_key, repo_state);
                            org_dirty = true;
                        }
                    }

                    if org_repos_update.is_some() || org_dirty {
                        pending_changes.push(RepoStateUpdate {
                            guild_id: *guild_id,
                            owner: owner.clone(),
                            repo: None,
                            is_org: true,
                            new_state: None,
                            org_repos: org_repos_update,
                            repo_states: Some(repo_states_update),
                        });
                    }
                } else if let Some(repo) = entry.repo.clone() {
                    let mut state = RepoTrackingData {
                        last_commit_shas: entry.last_commit_shas.clone(),
                        last_bug_closed_at: entry.last_bug_closed_at,
                        last_issue_updated_at: entry.last_issue_updated_at,
                    };

                    let (repo_updates, repo_dirty) = self
                        .poll_repository(*guild_id, entry.channel_id, &owner, &repo, &mut state)
                        .await?;

                    updates.extend(repo_updates);

                    if repo_dirty {
                        pending_changes.push(RepoStateUpdate {
                            guild_id: *guild_id,
                            owner: owner.clone(),
                            repo: Some(repo),
                            is_org: false,
                            new_state: Some(state),
                            org_repos: None,
                            repo_states: None,
                        });
                    }
                }
            }
        }

        if !pending_changes.is_empty() {
            let mut config = self.config.write().await;
            for change in pending_changes {
                if let Some(guild_entries) = config.guilds.get_mut(&change.guild_id) {
                    if let Some(entry) = guild_entries.iter_mut().find(|e| {
                        e.owner.eq_ignore_ascii_case(&change.owner)
                            && e.repo == change.repo
                            && e.is_org == change.is_org
                    }) {
                        if change.is_org {
                            if let Some(repos) = change.org_repos {
                                entry.org_repos = repos;
                            }
                            if let Some(states) = change.repo_states {
                                for (k, v) in states {
                                    entry.repo_data.insert(k, v);
                                }
                            }
                        } else if let Some(state) = change.new_state {
                            entry.last_commit_shas = state.last_commit_shas;
                            entry.last_bug_closed_at = state.last_bug_closed_at;
                            entry.last_issue_updated_at = state.last_issue_updated_at;
                        }
                    }
                }
            }
            self.store.save(&config).await?;
        }

        Ok(updates)
    }

    async fn poll_repository(
        &self,
        guild_id: u64,
        channel_id: u64,
        owner: &str,
        repo: &str,
        state: &mut RepoTrackingData,
    ) -> Result<(Vec<GithubUpdate>, bool), GithubError> {
        let mut updates = Vec::new();
        let mut dirty = false;

        let is_first_poll = state.last_commit_shas.is_empty();
        let branches = self.client.list_branches(owner, repo).await?;
        for branch in branches {
            let commits = self.client.list_commits(owner, repo, &branch, 10).await?;
            let latest_sha = commits.first().map(|c| c.sha.as_str());
            let last_seen_sha = state.last_commit_shas.get(&branch).cloned();

            if last_seen_sha.is_none() {
                if let Some(sha) = latest_sha {
                    if is_first_poll {
                        // Quiet baseline creation on first poll - just record the SHA
                        state
                            .last_commit_shas
                            .insert(branch.clone(), sha.to_string());
                        dirty = true;
                        continue;
                    }

                    // For new branches, find the first commit that's already tracked from another branch.
                    // This handles the case where branch B is created from branch A - we only want to
                    // report commits that are truly new, not the entire branch history.
                    let first_known_sha = commits.iter().find_map(|c| {
                        if state.last_commit_shas.values().any(|s| s == &c.sha) {
                            Some(c.sha.as_str())
                        } else {
                            None
                        }
                    });

                    // Record that we've now seen this branch
                    state
                        .last_commit_shas
                        .insert(branch.clone(), sha.to_string());
                    dirty = true;

                    if first_known_sha == Some(sha) {
                        // The latest commit is already tracked elsewhere, nothing new to report
                        continue;
                    }

                    // Report only commits newer than the first known one
                    let new_commits = collect_new_commits(&commits, first_known_sha);
                    if !new_commits.is_empty() {
                        for commit in &new_commits {
                            updates.push(GithubUpdate {
                                guild_id,
                                channel_id,
                                event: GithubEvent::CommitPushed {
                                    owner: owner.to_string(),
                                    repo: repo.to_string(),
                                    branch: branch.clone(),
                                    commit: commit.clone(),
                                },
                            });
                        }
                    }
                    continue;
                } else {
                    // Empty branch or error listing commits
                    continue;
                }
            }

            let new_commits = collect_new_commits(&commits, last_seen_sha.as_deref());

            if !new_commits.is_empty() {
                for commit in &new_commits {
                    updates.push(GithubUpdate {
                        guild_id,
                        channel_id,
                        event: GithubEvent::CommitPushed {
                            owner: owner.to_string(),
                            repo: repo.to_string(),
                            branch: branch.clone(),
                            commit: commit.clone(),
                        },
                    });
                }
                if let Some(latest) = commits.first() {
                    state
                        .last_commit_shas
                        .insert(branch.clone(), latest.sha.clone());
                    dirty = true;
                }
            }
        }

        // Closed bugs
        let bug_issues = self
            .client
            .list_bug_issues(owner, repo, state.last_bug_closed_at)
            .await?;
        let new_bugs = collect_closed_bugs(&bug_issues, state.last_bug_closed_at);
        if let Some(last_closed_at) = new_bugs
            .last()
            .and_then(|issue| issue.closed_at)
            .or_else(|| latest_closed_timestamp(&bug_issues))
        {
            if state.last_bug_closed_at != Some(last_closed_at) {
                state.last_bug_closed_at = Some(last_closed_at);
                dirty = true;
            }
        }
        for issue in new_bugs {
            updates.push(GithubUpdate {
                guild_id,
                channel_id,
                event: GithubEvent::BugClosed {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    issue,
                },
            });
        }

        // General issues (non-bug)
        let issues = self
            .client
            .list_general_issues(owner, repo, state.last_issue_updated_at)
            .await?;
        let new_issue_events = collect_issue_events(&issues, state.last_issue_updated_at);
        if let Some(latest) = issues
            .iter()
            .filter_map(|i| i.updated_at)
            .max()
            .or(state.last_issue_updated_at)
        {
            if state.last_issue_updated_at != Some(latest) {
                state.last_issue_updated_at = Some(latest);
                dirty = true;
            }
        }

        for (issue, activity) in new_issue_events {
            updates.push(GithubUpdate {
                guild_id,
                channel_id,
                event: GithubEvent::IssueActivity {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    issue,
                    activity,
                },
            });
        }

        Ok((updates, dirty))
    }
}

/// Collect only commits that happened after `last_seen`.
fn collect_new_commits(commits: &[Commit], last_seen: Option<&str>) -> Vec<Commit> {
    let mut new_commits = Vec::new();
    for commit in commits {
        if Some(commit.sha.as_str()) == last_seen {
            break;
        }
        new_commits.push(commit.clone());
    }
    new_commits.reverse(); // Oldest first for nicer Discord ordering
    new_commits
}

/// Pick the most recent closed bug time so we can update the watermark.
fn latest_closed_timestamp(issues: &[Issue]) -> Option<DateTime<Utc>> {
    issues.iter().filter_map(|i| i.closed_at).max()
}

/// Identify newly closed bug issues compared to the stored baseline.
fn collect_closed_bugs(
    issues: &[Issue],
    last_closed: Option<DateTime<Utc>>,
    // returns only closed bugs newer than baseline
) -> Vec<Issue> {
    let mut newly_closed = Vec::new();
    let baseline = last_closed.unwrap_or(DateTime::<Utc>::MIN_UTC);
    let first_run_cutoff = Utc::now() - Duration::minutes(30);

    for issue in issues {
        if issue.state != IssueState::Closed || !issue.is_bug {
            continue;
        }
        if let Some(closed_at) = issue.closed_at {
            let is_recent_first_run = last_closed.is_none() && closed_at >= first_run_cutoff;
            let is_newer_than_baseline = last_closed.is_some() && closed_at > baseline;

            if is_recent_first_run || is_newer_than_baseline {
                newly_closed.push(issue.clone());
            }
        }
    }

    newly_closed.sort_by_key(|i| i.closed_at.unwrap_or(baseline));
    newly_closed
}

/// Determine whether an issue event should be surfaced based on when we last checked.
fn collect_issue_events(
    issues: &[Issue],
    baseline: Option<DateTime<Utc>>,
) -> Vec<(Issue, IssueActivity)> {
    let mut events = Vec::new();
    let first_run_cutoff = Utc::now() - Duration::minutes(30);

    for issue in issues {
        if issue.is_bug {
            // Bug issues are handled separately to avoid duplicate embeds.
            continue;
        }

        let updated_at = issue.updated_at;
        let created_at = issue.created_at;

        match baseline {
            None => {
                // First run: only announce issues created recently to avoid spamming history.
                if let Some(created_at) = created_at {
                    if created_at >= first_run_cutoff {
                        events.push((issue.clone(), IssueActivity::Opened));
                    }
                }
            }
            Some(baseline) => {
                if let Some(updated_at) = updated_at {
                    if updated_at <= baseline {
                        continue;
                    }

                    let activity = match issue.state {
                        IssueState::Closed => IssueActivity::Closed,
                        IssueState::Open => match created_at {
                            Some(c) if (updated_at - c).num_seconds().abs() < 60 => {
                                IssueActivity::Opened
                            }
                            _ => IssueActivity::Updated,
                        },
                    };

                    events.push((issue.clone(), activity));
                }
            }
        }
    }

    events.sort_by_key(|(issue, _)| issue.updated_at.unwrap_or(DateTime::<Utc>::MIN_UTC));
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockGithubClient {
        branches: Vec<String>,
        commits: HashMap<String, Vec<Commit>>,
    }

    #[async_trait]
    impl GithubClient for MockGithubClient {
        async fn list_org_repos(&self, _org: &str) -> Result<Vec<String>, GithubError> {
            Ok(vec![])
        }
        async fn list_branches(
            &self,
            _owner: &str,
            _repo: &str,
        ) -> Result<Vec<String>, GithubError> {
            Ok(self.branches.clone())
        }
        async fn list_commits(
            &self,
            _owner: &str,
            _repo: &str,
            branch: &str,
            _per_page: usize,
        ) -> Result<Vec<Commit>, GithubError> {
            Ok(self.commits.get(branch).cloned().unwrap_or_default())
        }
        async fn list_bug_issues(
            &self,
            _owner: &str,
            _repo: &str,
            _since: Option<DateTime<Utc>>,
        ) -> Result<Vec<Issue>, GithubError> {
            Ok(vec![])
        }
        async fn list_general_issues(
            &self,
            _owner: &str,
            _repo: &str,
            _since: Option<DateTime<Utc>>,
        ) -> Result<Vec<Issue>, GithubError> {
            Ok(vec![])
        }
    }

    struct MockStore {
        config: Mutex<GithubConfig>,
    }

    #[async_trait]
    impl GithubConfigStore for MockStore {
        async fn load(&self) -> Result<GithubConfig, GithubError> {
            Ok(self.config.lock().unwrap().clone())
        }
        async fn save(&self, config: &GithubConfig) -> Result<(), GithubError> {
            *self.config.lock().unwrap() = config.clone();
            Ok(())
        }
    }

    fn create_commit(sha: &str) -> Commit {
        Commit {
            sha: sha.to_string(),
            message: "msg".to_string(),
            author_name: "auth".to_string(),
            html_url: "url".to_string(),
            avatar_url: None,
            committed_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn test_new_branch_detection() {
        let mut commits = HashMap::new();
        commits.insert("main".to_string(), vec![create_commit("sha1")]);

        let client = MockGithubClient {
            branches: vec!["main".to_string()],
            commits,
        };
        let store = MockStore {
            config: Mutex::new(GithubConfig::default()),
        };
        let service = GithubService::new(client, store).await.unwrap();

        // 1. Initial track
        service
            .track_repository(1, "owner", "repo", 100)
            .await
            .unwrap();

        // 2. First poll (baseline)
        let updates = service.poll_updates().await.unwrap();
        assert!(updates.is_empty(), "First poll should be quiet baseline");

        // 3. Add a new branch with a NEW commit
        let mut new_commits = HashMap::new();
        new_commits.insert("main".to_string(), vec![create_commit("sha1")]);
        new_commits.insert("feat".to_string(), vec![create_commit("sha2")]);

        let client_v2 = MockGithubClient {
            branches: vec!["main".to_string(), "feat".to_string()],
            commits: new_commits,
        };
        // Re-inject client (simulated by service update or new service with same store)
        let service_v2 = GithubService::new(client_v2, service.store).await.unwrap();

        let updates = service_v2.poll_updates().await.unwrap();
        assert_eq!(
            updates.len(),
            1,
            "Should detect 1 new commit on the new branch"
        );
        if let GithubEvent::CommitPushed { branch, commit, .. } = &updates[0].event {
            assert_eq!(branch, "feat");
            assert_eq!(commit.sha, "sha2");
        } else {
            panic!("Unexpected event type");
        }
    }

    #[tokio::test]
    async fn test_new_branch_from_main_no_new_commits_is_quiet() {
        let mut commits = HashMap::new();
        commits.insert("main".to_string(), vec![create_commit("sha1")]);

        let client = MockGithubClient {
            branches: vec!["main".to_string()],
            commits,
        };
        let store = MockStore {
            config: Mutex::new(GithubConfig::default()),
        };
        let service = GithubService::new(client, store).await.unwrap();

        service
            .track_repository(1, "owner", "repo", 100)
            .await
            .unwrap();
        service.poll_updates().await.unwrap();

        // Add a new branch pointing to the SAME commit
        let mut new_commits = HashMap::new();
        new_commits.insert("main".to_string(), vec![create_commit("sha1")]);
        new_commits.insert("feat".to_string(), vec![create_commit("sha1")]);

        let client_v2 = MockGithubClient {
            branches: vec!["main".to_string(), "feat".to_string()],
            commits: new_commits,
        };
        let service_v2 = GithubService::new(client_v2, service.store).await.unwrap();

        let updates = service_v2.poll_updates().await.unwrap();
        assert!(
            updates.is_empty(),
            "Should be quiet if the branch has no new unique commits"
        );
    }

    #[tokio::test]
    async fn test_branch_from_existing_only_reports_new_commits() {
        // Scenario: branch A has commits 1, 2, 3
        // Branch B is created from A and commits 4, 5 are made
        // Only commits 4 and 5 should be reported, NOT 1, 2, 3

        let mut commits = HashMap::new();
        // Main branch has commits sha3 (newest), sha2, sha1 (oldest)
        commits.insert(
            "main".to_string(),
            vec![
                create_commit("sha3"),
                create_commit("sha2"),
                create_commit("sha1"),
            ],
        );

        let client = MockGithubClient {
            branches: vec!["main".to_string()],
            commits,
        };
        let store = MockStore {
            config: Mutex::new(GithubConfig::default()),
        };
        let service = GithubService::new(client, store).await.unwrap();

        // Initial track and poll to establish baseline
        service
            .track_repository(1, "owner", "repo", 100)
            .await
            .unwrap();
        let updates = service.poll_updates().await.unwrap();
        assert!(updates.is_empty(), "First poll should be quiet");

        // Now add branch B created from main (sha3) with new commits sha5 (newest), sha4
        // Branch B's history: sha5, sha4, sha3, sha2, sha1
        let mut new_commits = HashMap::new();
        new_commits.insert(
            "main".to_string(),
            vec![
                create_commit("sha3"),
                create_commit("sha2"),
                create_commit("sha1"),
            ],
        );
        new_commits.insert(
            "feature-b".to_string(),
            vec![
                create_commit("sha5"),
                create_commit("sha4"),
                create_commit("sha3"), // This is where it branched from main
                create_commit("sha2"),
                create_commit("sha1"),
            ],
        );

        let client_v2 = MockGithubClient {
            branches: vec!["main".to_string(), "feature-b".to_string()],
            commits: new_commits,
        };
        let service_v2 = GithubService::new(client_v2, service.store).await.unwrap();

        let updates = service_v2.poll_updates().await.unwrap();

        // Should only report sha4 and sha5 (the 2 new commits), NOT sha1, sha2, sha3
        assert_eq!(
            updates.len(),
            2,
            "Should only detect 2 new commits (sha4, sha5), not the entire branch history"
        );

        // Verify the commits are sha4 and sha5 (in order oldest first)
        let shas: Vec<_> = updates
            .iter()
            .filter_map(|u| match &u.event {
                GithubEvent::CommitPushed { commit, .. } => Some(commit.sha.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            shas,
            vec!["sha4", "sha5"],
            "Should report sha4 and sha5 in order"
        );
    }
}
