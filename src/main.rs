// This is the entry point of the Discord bot.
//
// **Architecture Overview:**
// - `core/` = Business logic (platform-agnostic)
// - `infra/` = Implementations of core traits (databases, APIs)
// - `discord/` = Discord-specific adapters (commands, events)
//
// This file's job is to:
// 1. Load configuration
// 2. Initialize services (dependency injection)
// 3. Set up the Discord framework
// 4. Register commands and event handlers

// These attrs point each module declaration at a more descriptive root file
// so we don't end up with half a dozen mod.rs files that all look the same.
#[path = "core/core_layer.rs"]
mod core;
#[path = "discord/discord_layer.rs"]
mod discord;
#[path = "infra/infra_layer.rs"]
mod infra;

use crate::core::leveling::LevelingService;
use crate::core::server_stats::ServerStatsService;
use crate::discord::commands::presence;
use crate::discord::commands::server_stats::update_guild_stats;
use crate::discord::leveling_announcements::send_level_up_embed;
use crate::discord::{Data, Error};
use crate::infra::leveling::InMemoryXpStore;
use crate::infra::server_stats::JsonServerStatsStore;
use poise::serenity_prelude as serenity;

/// Event handler for non-command Discord events.
/// This is where we'll handle messages for XP gain.
async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Message { new_message } => {
            // Ignore bot messages (including our own)
            if new_message.author.bot {
                return Ok(());
            }

            // Only process guild messages (not DMs)
            if let Some(guild_id) = new_message.guild_id {
                let user_id = new_message.author.id.get();
                let guild_id = guild_id.get();

                // Try to award XP for this message
                match data.leveling.process_message(user_id, guild_id).await {
                    Ok(Some(level_up)) => {
                        tracing::info!(
                            user_id = level_up.user_id,
                            guild_id = level_up.guild_id,
                            old_level = level_up.old_level,
                            new_level = level_up.new_level,
                            total_xp = level_up.total_xp,
                            "User leveled up"
                        );

                        // User leveled up! Announce it
                        if let Err(err) = send_level_up_embed(ctx, new_message, data, &level_up).await {
                            tracing::warn!("Failed to send level-up embed: {err}");
                        }
                    }
                    Ok(None) => {
                        // XP was awarded but no level up - nothing to do
                    }
                    Err(crate::core::leveling::LevelingError::OnCooldown(_)) => {
                        // User is on cooldown - silently ignore
                    }
                    Err(e) => {
                        // Some other error - log it but don't crash
                        eprintln!("Error processing XP for message: {}", e);
                    }
                }
            }
        }
        serenity::FullEvent::GuildMemberAddition { new_member } => {
            if let Err(e) = update_guild_stats(ctx, data, new_member.guild_id).await {
                eprintln!("Error updating stats on join: {}", e);
            }
        }
        serenity::FullEvent::GuildMemberRemoval { guild_id, .. } => {
            if let Err(e) = update_guild_stats(ctx, data, *guild_id).await {
                eprintln!("Error updating stats on leave: {}", e);
            }
        }
        serenity::FullEvent::GuildUpdate { old_data_if_available: _, new_data } => {
             if let Err(e) = update_guild_stats(ctx, data, new_data.id).await {
                eprintln!("Error updating stats on guild update: {}", e);
            }
        }
        _ => {}
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialize logging so we can see what's happening
    tracing_subscriber::fmt::init();

    // Load environment variables from .env file (if it exists)
    dotenv::dotenv().ok();

    // Get Discord bot token from environment
    let token = std::env::var("DISCORD_TOKEN").expect(
        "Missing DISCORD_TOKEN environment variable! Create a .env file with your bot token.",
    );

    // ========================================================================
    // DEPENDENCY INJECTION
    // ========================================================================
    // Create our services with their dependencies.
    // This is the "composition root" where we wire everything together.

    // Create the in-memory XP store
    let xp_store = InMemoryXpStore::new();

    // Create the leveling service with the store injected
    let leveling_service = LevelingService::new(xp_store);

    // Create server stats store
    let stats_store = JsonServerStatsStore::new("server_stats.json");
    let stats_service = ServerStatsService::new(stats_store);

    // Create the data structure that will be shared across all commands
    let data = Data {
        leveling: leveling_service,
        server_stats: stats_service,
    };

    // ========================================================================
    // DISCORD FRAMEWORK SETUP
    // ========================================================================
    // Configure the poise framework with our commands and settings.

    let intents = serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT // Required to read message content
        | serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // Register all our commands here
            commands: vec![
                discord::commands::leveling::level(),
                discord::commands::leveling::leaderboard(),
                discord::commands::leveling::give_xp(),
                discord::commands::server_stats::serverstats(),
            ],
            // Event handler for messages and other events
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                println!("🤖 Bot is starting up...");

                // Register slash commands globally (can take up to an hour to propagate)
                // For faster development, use register_in_guild instead:
                // poise::builtins::register_in_guild(ctx, &framework.options().commands, guild_id).await?;
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                println!("✅ Commands registered!");
                println!("🚀 Bot is ready!");
                presence::on_ready(ctx, &data).await;

                Ok(data)
            })
        })
        .build();

    // Create the client and start the bot
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .expect("Error creating client");

    client.start().await.expect("Error running bot");
}
