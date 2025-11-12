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

mod core;
mod discord;
mod infra;

use crate::core::leveling::LevelingService;
use crate::discord::commands::presence;
use crate::discord::{Data, Error};
use crate::infra::leveling::InMemoryXpStore;
use poise::serenity_prelude as serenity;

/// Event handler for non-command Discord events.
/// This is where we'll handle messages for XP gain.
async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::Message { new_message } = event {
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
                    let _ = new_message
                        .reply(
                            &ctx.http,
                            format!(
                                "🎉 Congratulations <@{}>! Level {} ➜ **{}** ({} XP total)!",
                                new_message.author.id.get(),
                                level_up.old_level,
                                level_up.new_level,
                                level_up.total_xp
                            ),
                        )
                        .await;
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

    // Create the data structure that will be shared across all commands
    let data = Data {
        leveling: leveling_service,
    };

    // ========================================================================
    // DISCORD FRAMEWORK SETUP
    // ========================================================================
    // Configure the poise framework with our commands and settings.

    let intents = serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::MESSAGE_CONTENT // Required to read message content
        | serenity::GatewayIntents::GUILDS;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // Register all our commands here
            commands: vec![
                discord::commands::leveling::level(),
                discord::commands::leveling::leaderboard(),
                discord::commands::leveling::give_xp(),
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
                presence::on_ready(ctx);

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
