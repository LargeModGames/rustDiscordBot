use crate::core::server_stats::ServerStatsConfig;
use crate::discord::{Context, Data, Error};
use poise::serenity_prelude as serenity;

/// Manage server statistics channels
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    subcommands("setup", "remove", "status")
)]
pub async fn serverstats(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Set up server stats channels
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn setup(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?;
    let guild = ctx.guild().ok_or("Guild not found")?.clone();

    ctx.defer().await?;

    // Check if already configured
    if ctx
        .data()
        .server_stats
        .get_config(guild_id.get())
        .await?
        .is_some()
    {
        ctx.say("❌ Server stats are already configured for this server! Use `/serverstats remove` first if you want to re-configure.").await?;
        return Ok(());
    }

    // Create category
    let category = guild
        .create_channel(
            &ctx,
            serenity::CreateChannel::new("📊 SERVER STATS 📊")
                .kind(serenity::ChannelType::Category),
        )
        .await?;

    // Create channels
    // We need to deny connect permissions for @everyone
    let permissions = vec![serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::CONNECT,
        kind: serenity::PermissionOverwriteType::Role(serenity::RoleId::from(guild.id.get())), // @everyone role ID is same as guild ID
    }];

    let total_members = guild.member_count;
    // Note: Without GUILD_MEMBERS intent and full cache, these counts might be inaccurate
    // We'll do our best with what we have.
    // guild.member_count is reliable, but guild.members may be incomplete without the
    // GUILD_MEMBERS intent or a full cache. To ensure counts are still sensible, derive
    // the number of human members as `total_members - bots_cached`, where bots_cached is
    // the number of bots we can detect from the (partial) cache. This avoids reporting 0
    // members when the cache doesn't include most users.
    let bots_cached = guild.members.values().filter(|m| m.user.bot).count();
    let members = if total_members >= bots_cached as u64 {
        (total_members - bots_cached as u64) as usize
    } else {
        // Fallback in case the counts look inconsistent for some reason
        guild.members.values().filter(|m| !m.user.bot).count()
    };
    let bots = bots_cached;
    let boosts = guild.premium_subscription_count.unwrap_or(0);

    let total_members_channel = guild
        .create_channel(
            &ctx,
            serenity::CreateChannel::new(format!("🧑‍🤝‍🧑 All Members: {}", total_members))
                .kind(serenity::ChannelType::Voice)
                .category(category.id)
                .permissions(permissions.clone()),
        )
        .await?;

    let members_channel = guild
        .create_channel(
            &ctx,
            serenity::CreateChannel::new(format!("👤 Members: {}", members))
                .kind(serenity::ChannelType::Voice)
                .category(category.id)
                .permissions(permissions.clone()),
        )
        .await?;

    let bots_channel = guild
        .create_channel(
            &ctx,
            serenity::CreateChannel::new(format!("🤖 Bots: {}", bots))
                .kind(serenity::ChannelType::Voice)
                .category(category.id)
                .permissions(permissions.clone()),
        )
        .await?;

    let boost_channel = guild
        .create_channel(
            &ctx,
            serenity::CreateChannel::new(format!("🚀 Boosts: {}", boosts))
                .kind(serenity::ChannelType::Voice)
                .category(category.id)
                .permissions(permissions.clone()),
        )
        .await?;

    // Save config
    let config = ServerStatsConfig {
        guild_id: guild_id.get(),
        category_id: category.id.get(),
        total_members_channel_id: total_members_channel.id.get(),
        members_channel_id: members_channel.id.get(),
        bots_channel_id: bots_channel.id.get(),
        boost_channel_id: boost_channel.id.get(),
        enabled: true,
    };

    ctx.data().server_stats.save_config(config).await?;

    ctx.say("✅ Server stats channels have been set up successfully!")
        .await?;

    Ok(())
}

/// Remove server stats channels
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn remove(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?;

    ctx.defer().await?;

    let config = match ctx.data().server_stats.get_config(guild_id.get()).await? {
        Some(c) => c,
        None => {
            ctx.say("❌ Server stats are not configured for this server!")
                .await?;
            return Ok(());
        }
    };

    // Delete channels
    let channels = vec![
        config.total_members_channel_id,
        config.members_channel_id,
        config.bots_channel_id,
        config.boost_channel_id,
        config.category_id,
    ];

    for channel_id in channels {
        if let Ok(channel) = serenity::ChannelId::new(channel_id).to_channel(&ctx).await {
            if let Err(e) = channel.delete(&ctx).await {
                println!("Failed to delete channel {}: {}", channel_id, e);
            }
        }
    }

    // Remove from config
    ctx.data()
        .server_stats
        .delete_config(guild_id.get())
        .await?;

    ctx.say("✅ Server stats channels have been removed!")
        .await?;

    Ok(())
}

/// Show server stats status
#[poise::command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a guild")?;

    let config = match ctx.data().server_stats.get_config(guild_id.get()).await? {
        Some(c) => c,
        None => {
            ctx.say("❌ Server stats are not configured for this server!")
                .await?;
            return Ok(());
        }
    };

    let status = if config.enabled {
        "Enabled"
    } else {
        "Disabled"
    };

    let embed = serenity::CreateEmbed::default()
        .title("Server Stats Status")
        .color(0x3498db)
        .field("Status", status, false)
        .field("Category", format!("<#{}>", config.category_id), false)
        .field(
            "Total Members Channel",
            format!("<#{}>", config.total_members_channel_id),
            false,
        )
        .field(
            "Members Channel",
            format!("<#{}>", config.members_channel_id),
            false,
        )
        .field(
            "Bots Channel",
            format!("<#{}>", config.bots_channel_id),
            false,
        )
        .field(
            "Boosts Channel",
            format!("<#{}>", config.boost_channel_id),
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Helper function to update stats for a guild
pub async fn update_guild_stats(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> Result<(), Error> {
    let config = match data.server_stats.get_config(guild_id.get()).await? {
        Some(c) => c,
        None => return Ok(()), // Not configured
    };

    if !config.enabled {
        return Ok(());
    }

    // Fetch guild to get latest counts
    // We use the cache if possible, or fetch from API
    let guild = if let Some(g) = guild_id.to_guild_cached(&ctx.cache) {
        g.clone()
    } else {
        // If not in cache, we can't easily get member list without fetching
        // For now, just return if not in cache (it should be if we received an event)
        return Ok(());
    };

    let total_members = guild.member_count;

    // Note: `guild.members` is a HashMap in serenity 0.12 (FullGuild)
    let bots_count_cached = guild.members.values().filter(|m| m.user.bot).count();
    let members_count = if total_members >= bots_count_cached as u64 {
        (total_members - bots_count_cached as u64) as usize
    } else {
        guild.members.values().filter(|m| !m.user.bot).count()
    };
    let bots_count = bots_count_cached;
    let boosts = guild.premium_subscription_count.unwrap_or(0);

    // Update channels
    let _ = serenity::ChannelId::new(config.total_members_channel_id)
        .edit(
            &ctx,
            serenity::EditChannel::new().name(format!("🧑‍🤝‍🧑 All Members: {}", total_members)),
        )
        .await;

    let _ = serenity::ChannelId::new(config.members_channel_id)
        .edit(
            &ctx,
            serenity::EditChannel::new().name(format!("👤 Members: {}", members_count)),
        )
        .await;

    let _ = serenity::ChannelId::new(config.bots_channel_id)
        .edit(
            &ctx,
            serenity::EditChannel::new().name(format!("🤖 Bots: {}", bots_count)),
        )
        .await;

    let _ = serenity::ChannelId::new(config.boost_channel_id)
        .edit(
            &ctx,
            serenity::EditChannel::new().name(format!("🚀 Boosts: {}", boosts)),
        )
        .await;

    Ok(())
}
