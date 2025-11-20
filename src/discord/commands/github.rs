use crate::discord::commands::leveling::{Context, Error};
use crate::discord::github::dispatcher;
use poise::serenity_prelude as serenity;

/// Root `/github` command. Subcommands handle all configuration tasks.
#[poise::command(
    slash_command,
    guild_only,
    subcommands(
        "track",
        "track_org",
        "remove",
        "remove_org",
        "list",
        "check"
    )
)]
pub async fn github(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "GitHub tracking commands:\n\
        `/github track <owner> <repo>` - Track commits and issues for a repo\n\
        `/github track_org <org>` - Track every repo in an organization\n\
        `/github remove <owner> <repo>` - Stop tracking a repo\n\
        `/github remove_org <org>` - Stop tracking an organization\n\
        `/github list` - Show what is tracked in this guild\n\
        `/github check` - Force an immediate poll (admins only)",
    )
    .await?;
    Ok(())
}

/// Track a specific repository (all branches).
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn track(
    ctx: Context<'_>,
    #[description = "Repository owner (user or org)"] owner: String,
    #[description = "Repository name"] repo: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    ctx.data()
        .github
        .track_repository(guild_id, &owner, &repo, ctx.channel_id().get())
        .await?;

    ctx.say(format!(
        "Now tracking `{owner}/{repo}` (all branches) in this channel."
    ))
    .await?;
    Ok(())
}

/// Track every repository inside an organization.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn track_org(
    ctx: Context<'_>,
    #[description = "Organization login"] org: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let repos = ctx
        .data()
        .github
        .track_organization(guild_id, &org, ctx.channel_id().get())
        .await?;

    if repos.is_empty() {
        ctx.say(format!(
            "Could not find any repositories for `{org}`. Are you sure the org exists?"
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Now tracking organization `{org}` with {} repositories in this channel.",
            repos.len()
        ))
        .await?;
    }

    Ok(())
}

/// Stop tracking a single repository.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Repository owner (user or org)"] owner: String,
    #[description = "Repository name"] repo: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let removed = ctx
        .data()
        .github
        .remove_repository(guild_id, &owner, &repo)
        .await?;

    if removed {
        ctx.say(format!("Stopped tracking `{owner}/{repo}`.")).await?;
    } else {
        ctx.say("No matching repository entry found.").await?;
    }

    Ok(())
}

/// Stop tracking an organization.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn remove_org(
    ctx: Context<'_>,
    #[description = "Organization login"] org: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let removed = ctx
        .data()
        .github
        .remove_organization(guild_id, &org)
        .await?;

    if removed {
        ctx.say(format!("Stopped tracking organization `{org}`."))
            .await?;
    } else {
        ctx.say("No matching organization entry found.")
            .await?;
    }

    Ok(())
}

/// Show all tracked repositories and organizations for this guild.
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let entries = ctx.data().github.list_entries(guild_id).await;
    if entries.is_empty() {
        ctx.say("No repositories are being tracked in this guild.").await?;
        return Ok(());
    }

    let mut repo_lines = Vec::new();
    let mut org_lines = Vec::new();

    for entry in entries {
        if entry.is_org {
            org_lines.push(format!(
                "- `{}` ({} repos) -> <#{}>",
                entry.owner,
                entry.org_repos.len(),
                entry.channel_id
            ));
        } else if let Some(repo) = entry.repo {
            repo_lines.push(format!(
                "- `{}/{}` -> <#{}>",
                entry.owner, repo, entry.channel_id
            ));
        }
    }

    let mut description = String::new();
    if !org_lines.is_empty() {
        description.push_str("**Organizations:**\n");
        description.push_str(&org_lines.join("\n"));
        description.push('\n');
        description.push('\n');
    }
    if !repo_lines.is_empty() {
        description.push_str("**Repositories:**\n");
        description.push_str(&repo_lines.join("\n"));
    }

    let embed = serenity::CreateEmbed::new()
        .title("Tracked GitHub targets")
        .description(description)
        .color(serenity::Colour::from_rgb(88, 101, 242));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Force an immediate poll for this guild.
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn check(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let updates = ctx.data().github.poll_updates().await?;

    if updates.is_empty() {
        ctx.say("No new GitHub events detected.").await?;
    } else {
        dispatcher::send_updates(&ctx.serenity_context().http, updates).await;
        ctx.say("Posted new GitHub events.").await?;
    }

    Ok(())
}
