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
                !(entry.repo.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(repo))
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
    pub async fn remove_organization(
        &self,
        guild_id: u64,
        org: &str,
    ) -> Result<bool, GithubError> {
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
        let mut updated_config = snapshot.clone();
        let mut updates = Vec::new();
        let mut dirty = false;

        for (guild_id, entries) in updated_config.guilds.iter_mut() {
            for entry in entries.iter_mut() {
                let owner = entry.owner.clone();
                if entry.is_org {
                    if entry.org_repos.is_empty() {
                        entry.org_repos = self.client.list_org_repos(&owner).await?;
                        dirty = true;
                    }

                    for repo in entry.org_repos.clone() {
                        let repo_key = format!("{}/{}", owner, repo);
                        let repo_state = entry.repo_data.entry(repo_key.clone()).or_default();

                        let (repo_updates, repo_dirty) = self
                            .poll_repository(
                                *guild_id,
                                entry.channel_id,
                                &owner,
                                &repo,
                                repo_state,
                            )
                            .await?;
                        updates.extend(repo_updates);
                        dirty |= repo_dirty;
                    }
                } else if let Some(repo) = entry.repo.clone() {
                    let mut state = RepoTrackingData {
                        last_commit_shas: entry.last_commit_shas.clone(),
                        last_bug_closed_at: entry.last_bug_closed_at,
                        last_issue_updated_at: entry.last_issue_updated_at,
                    };

                    let (repo_updates, repo_dirty) = self
                        .poll_repository(
                            *guild_id,
                            entry.channel_id,
                            &owner,
                            &repo,
                            &mut state,
                        )
                        .await?;

                    updates.extend(repo_updates);
                    dirty |= repo_dirty;

                    if repo_dirty {
                        entry.last_commit_shas = state.last_commit_shas;
                        entry.last_bug_closed_at = state.last_bug_closed_at;
                        entry.last_issue_updated_at = state.last_issue_updated_at;
                    }
                }
            }
        }

        if dirty {
            let mut guard = self.config.write().await;
            *guard = updated_config.clone();
            self.store.save(&updated_config).await?;
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

        // Commits per branch
        let branches = self.client.list_branches(owner, repo).await?;
        for branch in branches {
            let commits = self
                .client
                .list_commits(owner, repo, &branch, 10)
                .await?;
            let last_seen = state.last_commit_shas.get(&branch).cloned();
            if last_seen.is_none() {
                // First run: store a baseline so we don't flood the channel with history.
                if let Some(latest) = commits.first() {
                    state
                        .last_commit_shas
                        .insert(branch.clone(), latest.sha.clone());
                    dirty = true;
                }
                continue;
            }

            let new_commits = collect_new_commits(&commits, last_seen.as_deref());

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
    issues
        .iter()
        .filter_map(|i| i.closed_at)
        .max()
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
                        IssueState::Open => {
                            match created_at {
                                Some(c) if (updated_at - c).num_seconds().abs() < 60 => {
                                    IssueActivity::Opened
                                }
                                _ => IssueActivity::Updated,
                            }
                        }
                    };

                    events.push((issue.clone(), activity));
                }
            }
        }
    }

    events.sort_by_key(|(issue, _)| issue.updated_at.unwrap_or(DateTime::<Utc>::MIN_UTC));
    events
}
