use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::core::github::{Commit, GithubClient, GithubError, Issue, IssueState};

/// Minimal GitHub REST API client. It deliberately exposes only the calls the core layer needs.
pub struct GithubApiClient {
    client: Client,
    base_url: String,
}

impl GithubApiClient {
    pub fn new(token: Option<String>) -> Result<Self, GithubError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("RustDiscordBot/1.0"),
        );
        if let Some(token) = token {
            headers.insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|e| GithubError::Api(e.to_string()))?,
            );
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| GithubError::Api(e.to_string()))?;

        Ok(Self {
            client,
            base_url: "https://api.github.com".to_string(),
        })
    }

    fn parse_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
        value
            .as_deref()
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    fn map_issue(api: ApiIssue, is_bug: bool) -> Issue {
        Issue {
            number: api.number.unwrap_or_default(),
            title: api.title.unwrap_or_else(|| "Untitled issue".to_string()),
            html_url: api
                .html_url
                .unwrap_or_else(|| "https://github.com".to_string()),
            reporter: api.user.and_then(|u| u.login),
            assignee: api.assignee.and_then(|a| a.login),
            closed_by: api.closed_by.and_then(|u| u.login),
            labels: api
                .labels
                .unwrap_or_default()
                .into_iter()
                .filter_map(|l| l.name)
                .collect(),
            state: match api.state.as_deref() {
                Some("closed") => IssueState::Closed,
                _ => IssueState::Open,
            },
            created_at: Self::parse_datetime(api.created_at),
            updated_at: Self::parse_datetime(api.updated_at),
            closed_at: Self::parse_datetime(api.closed_at),
            is_bug,
        }
    }

    async fn handle_rate_limit(&self, status: StatusCode) -> Result<(), GithubError> {
        if status == StatusCode::FORBIDDEN {
            return Err(GithubError::Api(
                "GitHub API rate limit hit or token missing permission".to_string(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl GithubClient for GithubApiClient {
    async fn list_org_repos(&self, org: &str) -> Result<Vec<String>, GithubError> {
        let url = format!("{}/orgs/{}/repos", self.base_url, org);
        let mut all_repos = Vec::new();

        for page in 1..=5 {
            let resp = self
                .client
                .get(&url)
                .query(&[("per_page", "100"), ("page", &page.to_string())])
                .send()
                .await
                .map_err(|e| GithubError::Api(e.to_string()))?;

            if resp.status() == StatusCode::NOT_FOUND {
                return Ok(Vec::new());
            }
            self.handle_rate_limit(resp.status()).await?;

            if resp.status().is_success() {
                let repos: Vec<ApiRepo> = resp
                    .json()
                    .await
                    .map_err(|e| GithubError::Api(e.to_string()))?;
                if repos.is_empty() {
                    break;
                }

                for repo in repos {
                    if let Some(name) = repo.name {
                        all_repos.push(name);
                    }
                }

                // Stop if we fetched the last page
                if all_repos.len() < page * 100 {
                    break;
                }
            } else {
                return Err(GithubError::Api(format!(
                    "GitHub returned {} for org repos",
                    resp.status()
                )));
            }
        }

        Ok(all_repos)
    }

    async fn list_branches(&self, owner: &str, repo: &str) -> Result<Vec<String>, GithubError> {
        let url = format!("{}/repos/{}/{}/branches", self.base_url, owner, repo);
        let resp = self
            .client
            .get(url)
            .query(&[("per_page", "100")])
            .send()
            .await
            .map_err(|e| GithubError::Api(e.to_string()))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        self.handle_rate_limit(resp.status()).await?;

        if resp.status().is_success() {
            let branches: Vec<ApiBranch> = resp
                .json()
                .await
                .map_err(|e| GithubError::Api(e.to_string()))?;
            Ok(branches
                .into_iter()
                .filter_map(|b| b.name)
                .collect::<Vec<_>>())
        } else {
            Err(GithubError::Api(format!(
                "Failed to fetch branches: {}",
                resp.status()
            )))
        }
    }

    async fn list_commits(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        per_page: usize,
    ) -> Result<Vec<Commit>, GithubError> {
        let url = format!("{}/repos/{}/{}/commits", self.base_url, owner, repo);
        let resp = self
            .client
            .get(url)
            .query(&[
                ("sha", branch),
                ("per_page", &per_page.to_string()),
            ])
            .send()
            .await
            .map_err(|e| GithubError::Api(e.to_string()))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        self.handle_rate_limit(resp.status()).await?;

        if resp.status().is_success() {
            let commits: Vec<ApiCommit> = resp
                .json()
                .await
                .map_err(|e| GithubError::Api(e.to_string()))?;

            Ok(commits
                .into_iter()
                .filter_map(|c| {
                    c.sha.map(|sha| Commit {
                        sha,
                        message: c
                            .commit
                            .as_ref()
                            .and_then(|c| c.message.clone())
                            .unwrap_or_else(|| "No commit message".to_string()),
                        author_name: c
                            .commit
                            .as_ref()
                            .and_then(|c| c.author.as_ref())
                            .and_then(|a| a.name.clone())
                            .or_else(|| c.author.as_ref().and_then(|a| a.login.clone()))
                            .unwrap_or_else(|| "Unknown author".to_string()),
                        html_url: c
                            .html_url
                            .unwrap_or_else(|| "https://github.com".to_string()),
                        avatar_url: c.author.and_then(|a| a.avatar_url),
                        committed_at: c
                            .commit
                            .and_then(|c| Self::parse_datetime(c.author.and_then(|a| a.date))),
                    })
                })
                .collect())
        } else {
            Err(GithubError::Api(format!(
                "Failed to fetch commits: {}",
                resp.status()
            )))
        }
    }

    async fn list_bug_issues(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Issue>, GithubError> {
        let url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
        let mut req = self
            .client
            .get(url)
            .query(&[
                ("state", "all"),
                ("labels", "bug"),
                ("sort", "updated"),
                ("direction", "desc"),
                ("per_page", "30"),
            ]);

        if let Some(since) = since {
            req = req.query(&[("since", &since.to_rfc3339())]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| GithubError::Api(e.to_string()))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        self.handle_rate_limit(resp.status()).await?;

        if resp.status().is_success() {
            let issues: Vec<ApiIssue> = resp
                .json()
                .await
                .map_err(|e| GithubError::Api(e.to_string()))?;
            Ok(issues
                .into_iter()
                .filter(|issue| issue.pull_request.is_none())
                .map(|issue| Self::map_issue(issue, true))
                .collect())
        } else {
            Err(GithubError::Api(format!(
                "Failed to fetch bug issues: {}",
                resp.status()
            )))
        }
    }

    async fn list_general_issues(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<Issue>, GithubError> {
        let url = format!("{}/repos/{}/{}/issues", self.base_url, owner, repo);
        let mut req = self
            .client
            .get(url)
            .query(&[
                ("state", "all"),
                ("sort", "updated"),
                ("direction", "desc"),
                ("per_page", "30"),
            ]);

        if let Some(since) = since {
            req = req.query(&[("since", &since.to_rfc3339())]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| GithubError::Api(e.to_string()))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        self.handle_rate_limit(resp.status()).await?;

        if resp.status().is_success() {
            let issues: Vec<ApiIssue> = resp
                .json()
                .await
                .map_err(|e| GithubError::Api(e.to_string()))?;
            Ok(issues
                .into_iter()
                .filter(|issue| issue.pull_request.is_none())
                .filter(|issue| {
                    !issue
                        .labels
                        .as_ref()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .any(|l| l.name.as_deref().unwrap_or("").eq_ignore_ascii_case("bug"))
                })
                .map(|issue| Self::map_issue(issue, false))
                .collect())
        } else {
            Err(GithubError::Api(format!(
                "Failed to fetch issues: {}",
                resp.status()
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiRepo {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiBranch {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiCommit {
    sha: Option<String>,
    html_url: Option<String>,
    commit: Option<ApiCommitInfo>,
    author: Option<ApiUser>,
}

#[derive(Debug, Deserialize)]
struct ApiCommitInfo {
    message: Option<String>,
    author: Option<ApiCommitAuthor>,
}

#[derive(Debug, Deserialize)]
struct ApiCommitAuthor {
    name: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiIssue {
    number: Option<u64>,
    title: Option<String>,
    html_url: Option<String>,
    user: Option<ApiUser>,
    assignee: Option<ApiUser>,
    closed_by: Option<ApiUser>,
    labels: Option<Vec<ApiLabel>>,
    state: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    closed_at: Option<String>,
    #[serde(default)]
    pull_request: Option<ApiPullRequestRef>,
}

#[derive(Debug, Deserialize)]
struct ApiPullRequestRef {}

#[derive(Debug, Deserialize)]
struct ApiLabel {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiUser {
    login: Option<String>,
    avatar_url: Option<String>,
}
