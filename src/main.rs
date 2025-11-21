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

use crate::core::ai::{AiConfig, AiService};
use crate::core::github::GithubService;
use crate::core::leveling::{LevelingService, MessageContentStats};
use crate::core::logging::{LoggingService, TrackedMessage};
use crate::core::server_stats::ServerStatsService;
use crate::core::timezones::TimezoneService;
use crate::discord::commands::presence;
use crate::discord::commands::server_stats::update_guild_stats;
use crate::discord::github::dispatcher as github_dispatcher;
use crate::discord::leveling_announcements::send_level_up_embed;
use crate::discord::logging::events as logging_events;
use crate::discord::{Data, Error};
use crate::infra::ai::OpenRouterClient;
use crate::infra::github::file_store::GithubFileStore;
use crate::infra::github::github_client::GithubApiClient;
use crate::infra::leveling::SqliteXpStore;
use crate::infra::logging::sqlite_store::SqliteLogStore;
use crate::infra::server_stats::JsonServerStatsStore;
use poise::serenity_prelude as serenity;

const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful AI assistant.";

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

            // Check for bot mention for AI response
            let bot_id = ctx.cache.current_user().id;
            if new_message.mentions.iter().any(|u| u.id == bot_id) {
                // It's a mention!
                // Trigger typing
                let _ = new_message.channel_id.broadcast_typing(&ctx.http).await;

                // Fetch recent messages for context
                // We want the last N messages, excluding the current one if possible, but Serenity's `messages`
                // usually returns the latest ones.
                // We'll fetch slightly more to be safe and filter.
                let max_history = std::env::var("OPENROUTER_MAX_HISTORY")
                    .ok()
                    .and_then(|v| v.parse::<u8>().ok())
                    .unwrap_or(50);

                let messages = new_message
                    .channel_id
                    .messages(&ctx.http, serenity::GetMessages::new().limit(max_history))
                    .await
                    .unwrap_or_default();

                // Convert to AiMessage, reversing order so it's oldest -> newest
                let mut context_messages = Vec::new();
                for msg in messages.iter().rev() {
                    // Skip the current message (the mention itself) if we want to handle it separately,
                    // or include it. Usually we include it as the last user message.
                    // But wait, `messages` includes the current message if we just fetch latest.

                    let role = if msg.author.id == bot_id {
                        "assistant".to_string()
                    } else {
                        "user".to_string()
                    };

                    let content = if role == "user" {
                        format!("{}: {}", msg.author.name, msg.content)
                    } else {
                        msg.content.clone()
                    };

                    context_messages.push(crate::core::ai::AiMessage { role, content });
                }

                // Call AI
                match data.ai.chat(&context_messages).await {
                    Ok(response) => {
                        // Send reasoning if present
                        if let Some(reasoning) = response.reasoning {
                            // Truncate reasoning if too long for embed description (4096 chars)
                            let mut reasoning_text = reasoning;
                            if reasoning_text.len() > 4000 {
                                reasoning_text.truncate(4000);
                                reasoning_text.push_str("...");
                            }

                            let embed = serenity::CreateEmbed::new()
                                .title("🧠 Reasoning")
                                .description(reasoning_text)
                                .color(0xDAA520) // Dark Gold
                                .footer(serenity::CreateEmbedFooter::new(
                                    "Generated by Greybeard Halt",
                                ));

                            if let Err(e) = new_message
                                .channel_id
                                .send_message(
                                    &ctx.http,
                                    serenity::CreateMessage::new().embed(embed),
                                )
                                .await
                            {
                                tracing::error!("Failed to send reasoning embed: {}", e);
                            }
                        }

                        // Split answer if too long (Discord limit 2000)
                        for chunk in response.answer.chars().collect::<Vec<char>>().chunks(2000) {
                            let chunk_str: String = chunk.iter().collect();
                            if let Err(e) = new_message.channel_id.say(&ctx.http, chunk_str).await {
                                tracing::error!("Failed to send AI response: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("AI error: {}", e);
                        let _ = new_message
                            .reply(
                                &ctx.http,
                                "Sorry, I encountered an error processing your request.",
                            )
                            .await;
                    }
                }
            }

            // Only process guild messages (not DMs)
            if let Some(guild_id) = new_message.guild_id {
                let user_id = new_message.author.id.get();
                let guild_id = guild_id.get();

                // Try to award XP for this message
                // Detect Nitro boosting (best-effort using cache). If unavailable, assume false.
                let boosted = ctx
                    .cache
                    .guild(serenity::GuildId::from(guild_id))
                    .and_then(|g| g.members.get(&serenity::UserId::from(user_id)).cloned())
                    .and_then(|m| m.premium_since)
                    .is_some();

                // Analyze message content
                let has_image = new_message.attachments.iter().any(|a| {
                    let name = a.filename.to_lowercase();
                    name.ends_with(".png")
                        || name.ends_with(".jpg")
                        || name.ends_with(".jpeg")
                        || name.ends_with(".gif")
                        || name.ends_with(".webp")
                });
                let is_long = new_message.content.len() >= 100;
                let has_link = new_message.content.contains("http://")
                    || new_message.content.contains("https://");

                let content_stats = MessageContentStats {
                    has_image,
                    is_long,
                    has_link,
                };

                match data
                    .leveling
                    .process_message(user_id, guild_id, boosted, Some(content_stats))
                    .await
                {
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
                        if let Err(err) =
                            send_level_up_embed(ctx, new_message, data, &level_up).await
                        {
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

            // Cache the message for logging so delete/edit events are reliable even when
            // Serenity's cache misses it.
            if let Some(guild_id) = new_message.guild_id {
                let tracked = TrackedMessage {
                    message_id: new_message.id.get(),
                    guild_id: guild_id.get(),
                    channel_id: new_message.channel_id.get(),
                    author_id: new_message.author.id.get(),
                    author_name: new_message.author.name.clone(),
                    content: new_message.content.clone(),
                    attachments: new_message
                        .attachments
                        .iter()
                        .map(|a| a.filename.clone())
                        .collect(),
                    avatar_url: new_message.author.avatar_url(),
                };

                data.logging.remember_message(tracked);
            }
        }
        serenity::FullEvent::GuildMemberAddition { new_member } => {
            if let Err(e) = update_guild_stats(ctx, data, new_member.guild_id).await {
                eprintln!("Error updating stats on join: {}", e);
            }
            if let Err(e) = logging_events::handle_member_join(ctx, data, new_member).await {
                tracing::error!("Error handling member join log: {}", e);
            }
        }
        serenity::FullEvent::GuildMemberRemoval {
            guild_id,
            user,
            member_data_if_available,
        } => {
            if let Err(e) = update_guild_stats(ctx, data, *guild_id).await {
                eprintln!("Error updating stats on leave: {}", e);
            }
            if let Err(e) = logging_events::handle_member_remove(
                ctx,
                data,
                *guild_id,
                user,
                member_data_if_available.as_ref(),
            )
            .await
            {
                tracing::error!("Error handling member remove log: {}", e);
            }
        }
        serenity::FullEvent::GuildUpdate {
            old_data_if_available: _,
            new_data,
        } => {
            if let Err(e) = update_guild_stats(ctx, data, new_data.id).await {
                eprintln!("Error updating stats on guild update: {}", e);
            }
        }
        serenity::FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
        } => {
            if let Err(e) = logging_events::handle_message_delete(
                ctx,
                data,
                *channel_id,
                *deleted_message_id,
                *guild_id,
            )
            .await
            {
                tracing::error!("Error handling message delete: {}", e);
            }
        }
        serenity::FullEvent::MessageUpdate {
            old_if_available,
            new,
            event,
        } => {
            if let Err(e) = logging_events::handle_message_update(
                ctx,
                data,
                old_if_available.as_ref(),
                new.as_ref(),
                event,
            )
            .await
            {
                tracing::error!("Error handling message update: {}", e);
            }
        }
        serenity::FullEvent::VoiceStateUpdate { old, new } => {
            if let Err(e) =
                logging_events::handle_voice_state_update(ctx, data, old.as_ref(), new).await
            {
                tracing::error!("Error handling voice state update: {}", e);
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

    // Keep runtime databases in a dedicated folder so the repo root stays tidy.
    let data_dir = "data";
    std::fs::create_dir_all(data_dir).expect("Failed to create data directory for SQLite files");
    let leveling_db_path = format!("{}/leveling.db", data_dir);
    let logging_db_path = format!("{}/logging.db", data_dir);

    // ========================================================================
    // DEPENDENCY INJECTION
    // ========================================================================
    // Create our services with their dependencies.
    // This is the "composition root" where we wire everything together.

    use std::sync::Arc;

    // Create the SQLite-backed XP store
    let xp_store = SqliteXpStore::new(&leveling_db_path)
        .await
        .expect("Failed to initialize SQLite store");

    // Create the leveling service with the store injected and wrap in Arc
    let leveling_service = Arc::new(LevelingService::new(xp_store));

    // Create server stats store
    let stats_store = JsonServerStatsStore::new("server_stats.json");
    let stats_service = Arc::new(ServerStatsService::new(stats_store));

    let timezone_service = Arc::new(TimezoneService::new());

    let log_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&format!("sqlite://{}?mode=rwc", logging_db_path))
        .await
        .expect("Failed to connect to logging DB");
    let log_store = SqliteLogStore::new(log_pool);
    log_store
        .migrate()
        .await
        .expect("Failed to migrate logging DB");
    let logging_service = Arc::new(LoggingService::new(log_store));

    // GitHub tracking service (polls commits/issues across repos)
    let github_token = std::env::var("GITHUB_TOKEN").ok();
    let github_client =
        GithubApiClient::new(github_token).expect("Failed to create GitHub API client");
    let github_store = GithubFileStore::new(format!("{}/github_config.json", data_dir));
    let github_service = Arc::new(
        GithubService::new(github_client, github_store)
            .await
            .expect("Failed to initialize GitHub tracking service"),
    );

    // AI Service
    let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("Missing OPENROUTER_API_KEY environment variable!");
    let openrouter_model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "deepseek/deepseek-chat-v3.1:free".to_string());
    let system_prompt = if let Ok(path) = std::env::var("OPENROUTER_SYSTEM_PROMPT_FILE") {
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            tracing::warn!("Failed to read system prompt file at {}: {}", path, e);
            DEFAULT_SYSTEM_PROMPT.to_string()
        })
    } else {
        std::env::var("OPENROUTER_SYSTEM_PROMPT")
            .unwrap_or_else(|_| DEFAULT_SYSTEM_PROMPT.to_string())
    };
    let reasoning_enabled = std::env::var("OPENROUTER_REASONING_ENABLED")
        .ok()
        .and_then(|v| v.parse::<bool>().ok());
    let reasoning_effort = std::env::var("OPENROUTER_REASONING_EFFORT").ok();
    let _max_history = std::env::var("OPENROUTER_MAX_HISTORY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50); // Default to 50 messages if not set

    let ai_client = OpenRouterClient::new(openrouter_api_key);
    let ai_config = AiConfig {
        model: openrouter_model,
        temperature: 0.7,
        max_tokens: None,
        top_p: Some(1.0),
        repetition_penalty: Some(1.0),
        reasoning_enabled,
        reasoning_effort,
    };
    let ai_service = Arc::new(AiService::new(ai_client, system_prompt, ai_config));

    // Create the data structure that will be shared across all commands
    let data = Data {
        leveling: Arc::clone(&leveling_service),
        server_stats: Arc::clone(&stats_service),
        timezones: Arc::clone(&timezone_service),
        logging: Arc::clone(&logging_service),
        github: Arc::clone(&github_service),
        ai: Arc::clone(&ai_service),
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
                discord::commands::leveling::profile(),
                discord::commands::leveling::xpstats(),
                discord::commands::leveling::next_achievement(),
                discord::commands::leveling::leaderboard(),
                discord::commands::leveling::give_xp(),
                discord::commands::leveling::daily_claim(),
                discord::commands::leveling::achievements(),
                discord::commands::server_stats::serverstats(),
                discord::commands::timezones::timezones(),
                crate::discord::logging::commands::logging(),
                discord::commands::github::github(),
            ],
            // Event handler for messages and other events
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            // Hook to run after every command
            post_command: |ctx| {
                Box::pin(async move {
                    if let Some(guild_id) = ctx.guild_id() {
                        let user_id = ctx.author().id.get();
                        let guild_id = guild_id.get();

                        // Increment command count and check achievements
                        if let Err(e) = ctx
                            .data()
                            .leveling
                            .increment_command_count(user_id, guild_id)
                            .await
                        {
                            tracing::error!("Failed to increment command count: {}", e);
                        }
                    }
                })
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

                // Register commands in the testing server to ensure they are always up to date immediately
                // NOTE: This causes duplicate commands in the testing server (one global, one guild-specific).
                // Commenting this out to avoid duplicates. If you need instant updates during dev, uncomment this
                // and comment out register_globally.

                // Explicitly clear guild commands to remove duplicates from previous runs
                poise::builtins::register_in_guild(
                    ctx,
                    &[] as &[poise::Command<Data, Error>], // Empty list clears guild commands
                    serenity::GuildId::new(1432001978447167611),
                )
                .await?;

                println!("✅ Commands registered!");
                println!("🚀 Bot is ready!");
                presence::on_ready(ctx, &data).await;

                // Background GitHub poller (commits, issues). Runs every 60 seconds.
                let github_service = Arc::clone(&data.github);
                let github_http = ctx.http.clone();
                tokio::spawn(async move {
                    use std::time::Duration as StdDuration;
                    use tokio::time::sleep;

                    loop {
                        tracing::debug!("Starting background GitHub poll...");
                        match github_service.poll_updates().await {
                            Ok(updates) => {
                                if !updates.is_empty() {
                                    tracing::info!("Found {} GitHub updates", updates.len());
                                    github_dispatcher::send_updates(&github_http, updates).await;
                                } else {
                                    tracing::debug!("No GitHub updates found");
                                }
                            }
                            Err(err) => tracing::warn!("GitHub poll failed: {}", err),
                        }

                        sleep(StdDuration::from_secs(60)).await;
                    }
                });

                // Spawn a background task to sweep guild members daily for booster tracking
                let leveling_clone = Arc::clone(&data.leveling);
                let http = ctx.http.clone();
                let cache = ctx.cache.clone();
                tokio::spawn(async move {
                    use std::time::Duration as StdDuration;
                    use tokio::time::sleep;

                    loop {
                        tracing::info!("Daily booster sweep starting");

                        // Refresh guild list every run using the cache to avoid missing new guilds
                        let guild_ids: Vec<u64> = cache.guilds().iter().map(|g| g.get()).collect();

                        for guild_id_u64 in guild_ids {
                            // Fetch members using the HTTP API to avoid sharing non-Send cache references between threads
                            if let Ok(members) = http
                                .get_guild_members(guild_id_u64.into(), None, Some(1000))
                                .await
                            {
                                for member in members {
                                    let user_id = member.user.id.get();
                                    let is_boosting = member.premium_since.is_some();
                                    if let Err(e) = leveling_clone
                                        .update_boost_status(user_id, guild_id_u64, is_boosting)
                                        .await
                                    {
                                        tracing::error!(
                                            "Failed to update boost status for {} in {}: {}",
                                            user_id,
                                            guild_id_u64,
                                            e
                                        );
                                    }
                                }
                            }
                        }

                        tracing::info!("Daily booster sweep completed");
                        // Wait 24 hours between sweeps (approx)
                        sleep(StdDuration::from_secs(60 * 60 * 24)).await;
                    }
                });

                Ok(data)
            })
        })
        .build();

    // Create the client and start the bot
    let mut settings = serenity::cache::Settings::default();
    settings.max_messages = 10000;

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .cache_settings(settings)
        .await
        .expect("Error creating client");

    client.start().await.expect("Error running bot");
}
