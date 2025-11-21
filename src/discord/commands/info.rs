use crate::discord::{Context, Error};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;

/// Provide quick access to Greybeard Game Studios background information.
#[poise::command(slash_command, prefix_command)]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id();
    let embed = build_info_embed(ctx.serenity_context(), guild_id).await;
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

pub async fn build_info_embed(
    ctx: &serenity::Context,
    guild_id: Option<serenity::GuildId>,
) -> serenity::CreateEmbed {
    let mut apply_channel_mention = "#apply-here".to_string();
    let mut role_overview_mention = "#role-overview".to_string();

    if let Some(guild_id) = guild_id {
        // Try to find channels in the guild
        // We prefer cache, but fallback to HTTP if needed (though cache should have it)
        let cached_channels = if let Some(guild) = ctx.cache.guild(guild_id) {
            Some(guild.channels.clone())
        } else {
            None
        };

        let channels = if let Some(c) = cached_channels {
            Some(c)
        } else {
            guild_id.channels(&ctx.http).await.ok()
        };

        if let Some(channels) = channels {
            for channel in channels.values() {
                if channel.name == "apply-here" {
                    apply_channel_mention = channel.mention().to_string();
                } else if channel.name == "role-overview" {
                    role_overview_mention = channel.mention().to_string();
                }
            }
        }
    }

    let mut embed = serenity::CreateEmbed::new()
        .title("Greybeard Game Studios — Start Here")
        .description("Everything you need to introduce new members to the studio, our current project, and how to get involved.")
        .color(0x5865F2) // discord::Color::from_rgb(88, 101, 242)
        .timestamp(serenity::Timestamp::now());

    // Set thumbnail to bot avatar
    if let Ok(bot_user) = ctx.http.get_current_user().await {
        if let Some(avatar_url) = bot_user.avatar_url() {
            embed = embed.thumbnail(avatar_url);
        }
    }

    embed = embed.field(
        "Who We Are",
        "Greybeard Game Studios is an indie collective led by Ranger-Z with community managers LargeModGames and Att keeping everything running. We're a remote-first team united by a love of deep, story-rich RPGs.",
        false,
    );

    embed = embed.field(
        "What We Build",
        "We're crafting *Project Fiefdom*, a narrative-driven single-player RPG about loyalty, betrayal, and surviving the harsh realities of a living feudal world. Programmers, artists, writers, and audio leads collaborate daily to bring each kingdom faction to life.",
        false,
    );

    embed = embed.field(
        "How to Apply",
        format!(
            "1. Visit {} after reading the guidelines in {}.\n\
             2. Post one application per role and include:\n\
             \u{00A0}\u{00A0}• The specific role you're pursuing\n\
             \u{00A0}\u{00A0}• Past experience plus portfolio or resume links\n\
             \u{00A0}\u{00A0}• Your weekly availability (e.g., 10 hours/week)\n\
             3. Acknowledge ping expectations—respond within 48 hours or you'll be removed and must reapply.\n\
             4. All newcomers complete a mandatory one-week probation so we can assess fit.\n\
             By applying you agree to the legal agreement and project rules attached in {}.",
            apply_channel_mention, role_overview_mention, apply_channel_mention
        ),
        false,
    );

    embed = embed.field(
        "Need Help?",
        "Have questions before applying? Ping LargeModGames or Att, or drop a note in the community help channels. We're happy to review portfolios, point you at resources, or chat about where you can plug in.",
        false,
    );

    embed.footer(serenity::CreateEmbedFooter::new(
        "Welcome aboard! let's build something unforgettable together.",
    ))
}
