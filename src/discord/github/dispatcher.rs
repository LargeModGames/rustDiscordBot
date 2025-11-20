use crate::core::github::{GithubEvent, GithubUpdate, IssueActivity};
use poise::serenity_prelude as serenity;

/// Send all GitHub updates to their target channels with user-friendly embeds.
pub async fn send_updates(http: &serenity::Http, updates: Vec<GithubUpdate>) {
    for update in updates {
        if let Err(err) = send_single(http, &update).await {
            tracing::warn!(
                guild_id = update.guild_id,
                channel_id = update.channel_id,
                error = %err,
                "Failed to send GitHub update"
            );
        }
    }
}

async fn send_single(
    http: &serenity::Http,
    update: &GithubUpdate,
) -> Result<(), serenity::Error> {
    let channel_id = serenity::ChannelId::new(update.channel_id);
    let embed = match &update.event {
        GithubEvent::CommitPushed {
            owner,
            repo,
            branch,
            commit,
        } => build_commit_embed(owner, repo, branch, commit),
        GithubEvent::BugClosed { owner, repo, issue } => {
            build_bug_embed(owner, repo, issue)
        }
        GithubEvent::IssueActivity {
            owner,
            repo,
            issue,
            activity,
        } => build_issue_embed(owner, repo, issue, *activity),
    };

    channel_id
        .send_message(http, serenity::CreateMessage::new().embed(embed))
        .await?;
    Ok(())
}

fn build_commit_embed(
    owner: &str,
    repo: &str,
    branch: &str,
    commit: &crate::core::github::Commit,
) -> serenity::CreateEmbed {
    let short_sha = commit.sha.chars().take(7).collect::<String>();
    let first_line = commit
        .message
        .lines()
        .next()
        .unwrap_or("No commit message")
        .to_string();

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("[{repo}:{branch}] new commit"))
        .description(format!(
            "[`{}`]({}) {}",
            short_sha, commit.html_url, first_line
        ))
        .color(serenity::Colour::from_rgb(88, 101, 242))
        .timestamp(serenity::Timestamp::now())
        .footer(serenity::CreateEmbedFooter::new(format!("{owner}/{repo}")));

    if let Some(avatar) = &commit.avatar_url {
        embed = embed.author(
            serenity::CreateEmbedAuthor::new(&commit.author_name).icon_url(avatar.clone()),
        );
    } else {
        embed = embed.author(serenity::CreateEmbedAuthor::new(&commit.author_name));
    }

    if let Some(committed_at) = format_dt(commit.committed_at) {
        embed = embed.field("Committed at", committed_at, true);
    }

    embed
}

fn build_bug_embed(
    owner: &str,
    repo: &str,
    issue: &crate::core::github::Issue,
) -> serenity::CreateEmbed {
    let number = issue.number;
    let closed_at = format_dt(issue.closed_at);

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("Bug fixed: #{number}"))
        .description(format!("[{}]({})", issue.title, issue.html_url))
        .color(serenity::Colour::from_rgb(67, 181, 129))
        .timestamp(serenity::Timestamp::now())
        .footer(serenity::CreateEmbedFooter::new(format!("{owner}/{repo}")));

    if let Some(reporter) = &issue.reporter {
        embed = embed.field("Opened by", format!("`{reporter}`"), true);
    }
    if let Some(closed_by) = &issue.closed_by {
        embed = embed.field("Closed by", format!("`{closed_by}`"), true);
    }
    if let Some(closed_at) = closed_at {
        embed = embed.field("Closed at", closed_at, true);
    }
    if !issue.labels.is_empty() {
        let labels = issue.labels.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        embed = embed.field("Labels", labels, false);
    }

    embed
}

fn build_issue_embed(
    owner: &str,
    repo: &str,
    issue: &crate::core::github::Issue,
    activity: IssueActivity,
) -> serenity::CreateEmbed {
    let number = issue.number;
    let status = match activity {
        IssueActivity::Opened => "opened",
        IssueActivity::Updated => "updated",
        IssueActivity::Closed => "closed",
    };

    let color = match activity {
        IssueActivity::Opened => serenity::Colour::from_rgb(67, 181, 129),
        IssueActivity::Updated => serenity::Colour::GOLD,
        IssueActivity::Closed => serenity::Colour::RED,
    };

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("Issue #{number} {status}"))
        .description(format!("[{}]({})", issue.title, issue.html_url))
        .color(color)
        .timestamp(serenity::Timestamp::now())
        .footer(serenity::CreateEmbedFooter::new(format!("{owner}/{repo}")));

    if let Some(reporter) = &issue.reporter {
        embed = embed.field("Author", format!("`{reporter}`"), true);
    }
    if let Some(assignee) = &issue.assignee {
        embed = embed.field("Assigned to", format!("`{assignee}`"), true);
    }
    if activity == IssueActivity::Closed {
        if let Some(closed_at) = format_dt(issue.closed_at) {
            embed = embed.field("Closed at", closed_at, true);
        }
    } else if let Some(updated_at) = format_dt(issue.updated_at) {
        embed = embed.field("Updated at", updated_at, true);
    }

    if !issue.labels.is_empty() {
        let labels = issue.labels.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        embed = embed.field("Labels", labels, false);
    }

    embed
}

fn format_dt(dt: Option<chrono::DateTime<chrono::Utc>>) -> Option<String> {
    dt.map(|d| format!("<t:{}:F>", d.timestamp()))
}
