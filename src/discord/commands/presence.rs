// This module handles bot presence and lifecycle events.
//
// Everything here is Discord-layer glue that adapts user-facing presence
// updates into the primitives expected by the core. That means we only work
// with Discord SDK types (Context, ActivityData, OnlineStatus) and keep the
// logic extremely short and intention-revealing.

use poise::serenity_prelude as serenity;

/// Updates the bot's status to show the currently playing song.
#[allow(dead_code)] // Will be used by the music service once implemented
pub fn change_status(ctx: &serenity::Context, song_name: &str) {
    // serenity 0.12 exposes builder helpers on ActivityData, not Activity.
    // Using the helper keeps us on the public API surface and documents the
    // semantic intent ("Listening to...") instead of manually constructing
    // the ActivityData struct every time.
    let activity = serenity::ActivityData::listening(format!("Listening to: {}", song_name));
    ctx.set_presence(Some(activity), serenity::OnlineStatus::Online);
}

/// Resets the bot's status to the default message.
pub fn reset_status(ctx: &serenity::Context) {
    let activity = serenity::ActivityData::playing("Building Project Fiefdom");
    ctx.set_presence(Some(activity), serenity::OnlineStatus::Online);
}

/// Called when the bot is ready.
pub async fn on_ready(ctx: &serenity::Context, data: &crate::discord::Data) {
    println!("Bot is ready!");
    reset_status(ctx);

    // Update server stats channels on startup for all configured guilds
    match data.server_stats.get_all_configs().await {
        Ok(configs) => {
            for cfg in configs {
                let guild_id = serenity::GuildId::from(cfg.guild_id);
                if let Err(e) =
                    crate::discord::commands::server_stats::update_guild_stats(
                        ctx,
                        data,
                        guild_id,
                        crate::discord::commands::server_stats::StatsUpdateEvent::None,
                    )
                        .await
                {
                    eprintln!("Failed to update stats for guild {}: {}", cfg.guild_id, e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to load server stats configs: {}", e);
        }
    }
}
