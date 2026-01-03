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

use crate::core::ai::models::AiTool;
use crate::core::ai::{AiConfig, AiService, FunctionCallHandler};
use crate::core::economy::EconomyService;
use crate::core::github::GithubService;
use crate::core::leveling::{LevelingService, MessageContentStats};
use crate::core::logging::{LoggingService, TrackedMessage};
use crate::core::server_stats::ServerStatsService;
use crate::core::timezones::TimezoneService;
use crate::discord::commands::presence;
use crate::discord::commands::server_stats::{update_guild_stats, StatsUpdateEvent};
use crate::discord::github::dispatcher as github_dispatcher;
use crate::discord::leveling_announcements::send_level_up_embed;
use crate::discord::logging::events as logging_events;
use crate::discord::{Data, Error};
use crate::infra::ai::{GeminiClient, OpenRouterClient};
use crate::infra::economy::SqliteCoinStore;
use crate::infra::github::file_store::GithubFileStore;
use crate::infra::github::github_client::GithubApiClient;
use crate::infra::google_docs::GoogleDocsFunctionHandler;
use crate::infra::leveling::SqliteXpStore;
use crate::infra::logging::sqlite_store::SqliteLogStore;
use crate::infra::server_stats::JsonServerStatsStore;
use poise::serenity_prelude as serenity;
use std::str::FromStr;

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

            // Anti-spam check (before any other processing)
            // If detected as spam, the handler will delete/warn/timeout as needed
            if let Ok(is_spam) = discord::moderation::spam_handler::handle_message_for_spam(
                ctx,
                new_message,
                data.anti_spam.as_ref(),
            )
            .await
            {
                if is_spam {
                    // Message was spam - don't process further (no XP, no AI, etc.)
                    return Ok(());
                }
            }

            // Check for bot mention for AI response
            let bot_id = ctx.cache.current_user().id;
            if new_message.mentions.iter().any(|u| u.id == bot_id) {
                // Check if it's a question about the project
                let content_lower = new_message.content.to_lowercase();
                let is_project_question = content_lower.contains("project")
                    || content_lower.contains("fiefdom")
                    || content_lower.contains("greybeard")
                    || content_lower.contains("studio")
                    || content_lower.contains("apply")
                    || content_lower.contains("application")
                    || content_lower.contains("join")
                    || (content_lower.contains("what") && content_lower.contains("building"))
                    || (content_lower.contains("who") && content_lower.contains("are you"));

                if is_project_question {
                    let embed =
                        crate::discord::commands::info::build_info_embed(ctx, new_message.guild_id)
                            .await;
                    if let Err(e) = new_message
                        .channel_id
                        .send_message(&ctx.http, serenity::CreateMessage::new().embed(embed))
                        .await
                    {
                        tracing::error!("Failed to send info embed: {}", e);
                    }
                    // If we answered with the info embed, we skip the AI response to avoid double-replying
                    return Ok(());
                }

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

                // First, fetch background context from announcement/sneak-peek channels.
                // This gives the AI knowledge about the project even if the current
                // conversation doesn't mention those details.
                let mut context_messages =
                    crate::discord::ai::fetch_context_channels(&ctx.http, 10).await;

                // Add a separator between background context and current conversation
                if !context_messages.is_empty() {
                    context_messages.push(crate::core::ai::AiMessage {
                        role: "system".to_string(),
                        content: "--- Current conversation ---".to_string(),
                    });
                }

                let messages = new_message
                    .channel_id
                    .messages(&ctx.http, serenity::GetMessages::new().limit(max_history))
                    .await
                    .unwrap_or_default();

                // Convert to AiMessage, reversing order so it's oldest -> newest
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
                        // Include the user's display name AND their mention format
                        // so the AI can ping them if needed
                        format!(
                            "{} (ping: <@{}>): {}",
                            msg.author.name, msg.author.id, msg.content
                        )
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

                // Try to award random coins (silent - no announcement)
                if let Err(e) = data
                    .economy
                    .try_random_message_reward(user_id, guild_id)
                    .await
                {
                    tracing::debug!("Failed to award random message coins: {}", e);
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
            if let Err(e) = update_guild_stats(
                ctx,
                data,
                new_member.guild_id,
                StatsUpdateEvent::MemberJoin(new_member),
            )
            .await
            {
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
            if let Err(e) =
                update_guild_stats(ctx, data, *guild_id, StatsUpdateEvent::MemberLeave(user)).await
            {
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
            if let Err(e) = update_guild_stats(
                ctx,
                data,
                new_data.id,
                StatsUpdateEvent::GuildUpdate(new_data),
            )
            .await
            {
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
    let config_dir = "config";
    std::fs::create_dir_all(config_dir).expect("Failed to create config directory");
    let stats_store = JsonServerStatsStore::new(format!("{}/server_stats.json", config_dir));
    let stats_service = Arc::new(ServerStatsService::new(stats_store));

    let timezone_service = Arc::new(TimezoneService::new());

    let log_conn_str = format!("sqlite://{}", logging_db_path);
    let log_options = sqlx::sqlite::SqliteConnectOptions::from_str(&log_conn_str)
        .expect("Invalid connection string")
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let log_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(log_options)
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
    // -------------------------------------------------------------------------
    // The bot supports two AI providers:
    // 1. OpenRouter (default) - Access to many models via openrouter.ai
    // 2. Gemini - Google's Gemini API via ai.google.dev
    //
    // Set AI_PROVIDER=gemini to use Gemini, otherwise OpenRouter is used.
    // -------------------------------------------------------------------------
    let ai_provider = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "openrouter".to_string());

    // Load system prompt (shared between providers)
    let system_prompt = if let Ok(path) = std::env::var("AI_SYSTEM_PROMPT_FILE") {
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            tracing::warn!("Failed to read system prompt file at {}: {}", path, e);
            DEFAULT_SYSTEM_PROMPT.to_string()
        })
    } else {
        std::env::var("AI_SYSTEM_PROMPT")
            .or_else(|_| std::env::var("OPENROUTER_SYSTEM_PROMPT")) // Backwards compat
            .unwrap_or_else(|_| DEFAULT_SYSTEM_PROMPT.to_string())
    };

    // Build AI service based on provider
    let ai_service: Arc<AiService<Box<dyn crate::core::ai::AiProvider>>> = if ai_provider
        .to_lowercase()
        == "gemini"
    {
        // Gemini configuration
        let gemini_api_key = std::env::var("GEMINI_API_KEY")
            .expect("Missing GEMINI_API_KEY environment variable when AI_PROVIDER=gemini");
        let mut gemini_model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "auto".to_string());

        // Handle "auto" or "best" to always use the top of our hierarchy
        if gemini_model == "auto" || gemini_model == "best" {
            gemini_model = crate::core::ai::models::MODEL_HIERARCHY[0].to_string();
        }

        tracing::info!("Using Gemini AI provider with model: {}", gemini_model);

        // Set up Google Docs function handler
        // Try to use service account auth for multi-tab support, fall back to public export
        let handler = match GoogleDocsFunctionHandler::from_env_with_auth().await {
            Ok(h) => {
                tracing::info!(
                    "Google Docs: Using service account authentication (multi-tab support enabled)"
                );
                h
            }
            Err(e) => {
                tracing::info!("Google Docs: Service account not configured ({}), using public export (first tab only)", e);
                GoogleDocsFunctionHandler::from_env()
            }
        };

        // Check if any project docs are configured
        let has_project_docs = handler.supported_functions().len() > 1; // More than just read_google_doc

        // Enable Google Search + Google Docs reading
        let enable_search = std::env::var("AI_ENABLE_GOOGLE_SEARCH")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true); // Default to enabled

        let (tools, function_handler): (
            Option<Vec<AiTool>>,
            Option<Box<dyn crate::core::ai::FunctionCallHandler>>,
        ) = {
            let tools = handler.get_tools(enable_search);
            tracing::info!(
                "Gemini tools enabled: Google Search={}, Google Docs functions={}",
                enable_search,
                handler.supported_functions().join(", ")
            );

            if has_project_docs {
                tracing::info!("Project documents configured for AI access");
            }

            (
                Some(tools),
                Some(Box::new(handler) as Box<dyn crate::core::ai::FunctionCallHandler>),
            )
        };

        let gemini_client = GeminiClient::new(gemini_api_key);
        let ai_config = AiConfig {
            model: gemini_model,
            temperature: std::env::var("AI_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.7),
            max_tokens: std::env::var("AI_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok()),
            top_p: std::env::var("AI_TOP_P")
                .ok()
                .and_then(|v| v.parse().ok())
                .or(Some(1.0)),
            repetition_penalty: None, // Not supported by Gemini
            reasoning_enabled: std::env::var("AI_REASONING_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok()),
            reasoning_effort: std::env::var("AI_REASONING_EFFORT").ok(),
            tools,
            tool_config: None, // Default tool behavior (AUTO)
        };

        // Create AI service with or without function handler
        match function_handler {
            Some(handler) => Arc::new(AiService::with_function_handler(
                Box::new(gemini_client) as Box<dyn crate::core::ai::AiProvider>,
                system_prompt,
                ai_config,
                handler,
            )),
            None => Arc::new(AiService::new(
                Box::new(gemini_client) as Box<dyn crate::core::ai::AiProvider>,
                system_prompt,
                ai_config,
            )),
        }
    } else {
        // OpenRouter configuration (default)
        let openrouter_api_key = std::env::var("OPENROUTER_API_KEY")
            .expect("Missing OPENROUTER_API_KEY environment variable!");
        let openrouter_model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "deepseek/deepseek-chat-v3.1:free".to_string());

        tracing::info!(
            "Using OpenRouter AI provider with model: {}",
            openrouter_model
        );

        let reasoning_enabled = std::env::var("OPENROUTER_REASONING_ENABLED")
            .or_else(|_| std::env::var("AI_REASONING_ENABLED"))
            .ok()
            .and_then(|v| v.parse::<bool>().ok());
        let reasoning_effort = std::env::var("OPENROUTER_REASONING_EFFORT")
            .or_else(|_| std::env::var("AI_REASONING_EFFORT"))
            .ok();

        let ai_client = OpenRouterClient::new(openrouter_api_key);
        let ai_config = AiConfig {
            model: openrouter_model,
            temperature: std::env::var("AI_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.7),
            max_tokens: std::env::var("AI_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok()),
            top_p: std::env::var("AI_TOP_P")
                .ok()
                .and_then(|v| v.parse().ok())
                .or(Some(1.0)),
            repetition_penalty: Some(1.0),
            reasoning_enabled,
            reasoning_effort,
            tools: None, // OpenRouter: limited tool support depends on model
            tool_config: None,
        };

        Arc::new(AiService::new(
            Box::new(ai_client) as Box<dyn crate::core::ai::AiProvider>,
            system_prompt,
            ai_config,
        ))
    };

    let _max_history = std::env::var("AI_MAX_HISTORY")
        .or_else(|_| std::env::var("OPENROUTER_MAX_HISTORY")) // Backwards compat
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50);

    // Economy Service
    let economy_db_path = format!("{}/economy.db", data_dir);
    let coin_store = SqliteCoinStore::new(&economy_db_path)
        .await
        .expect("Failed to initialize economy store");
    let economy_service = Arc::new(EconomyService::new(coin_store));

    // Inventory Service (uses same SQLite pool as economy for shared schema)
    let inventory_conn_str = format!("sqlite://{}", economy_db_path);
    let inventory_options = sqlx::sqlite::SqliteConnectOptions::from_str(&inventory_conn_str)
        .expect("Invalid connection string")
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let inventory_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(inventory_options)
        .await
        .expect("Failed to connect to inventory DB");

    let inventory_store = crate::infra::economy::SqliteInventoryStore::new(inventory_pool);
    let inventory_service = Arc::new(crate::core::economy::InventoryService::new(inventory_store));

    // Anti-Spam Moderation Service
    let moderation_db_path = format!("{}/moderation.db", data_dir);
    let moderation_conn_str = format!("sqlite://{}", moderation_db_path);
    let moderation_options = sqlx::sqlite::SqliteConnectOptions::from_str(&moderation_conn_str)
        .expect("Invalid moderation DB connection string")
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let moderation_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(moderation_options)
        .await
        .expect("Failed to connect to moderation DB");

    let spam_store = crate::infra::moderation::SqliteSpamStore::new(moderation_pool);
    spam_store
        .migrate()
        .await
        .expect("Failed to migrate moderation DB");
    let anti_spam_service = Arc::new(crate::core::moderation::AntiSpamService::new(spam_store));

    // Create the data structure that will be shared across all commands
    let data = Data {
        leveling: Arc::clone(&leveling_service),
        server_stats: Arc::clone(&stats_service),
        timezones: Arc::clone(&timezone_service),
        logging: Arc::clone(&logging_service),
        github: Arc::clone(&github_service),
        ai: Arc::clone(&ai_service),
        economy: Arc::clone(&economy_service),
        inventory: Arc::clone(&inventory_service),
        anti_spam: Arc::clone(&anti_spam_service),
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
                discord::commands::leveling::achievements(),
                discord::commands::leveling::prestige(),
                discord::commands::leveling::sync_prestige(),
                discord::commands::economy::balance(),
                discord::commands::economy::daily(),
                discord::commands::shop::shop(),
                discord::commands::shop::inventory(),
                discord::commands::server_stats::serverstats(),
                discord::commands::timezones::timezones(),
                crate::discord::logging::commands::logging(),
                discord::commands::github::github(),
                discord::commands::info::info(),
                discord::commands::help::help(),
                // Anti-spam moderation
                discord::moderation::commands::antispam(),
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
                // poise::builtins::register_in_guild(
                //    ctx,
                //    &[] as &[poise::Command<Data, Error>], // Empty list clears guild commands
                //    serenity::GuildId::new(1432001978447167611),
                // )
                // .await?;

                println!("✅ Commands registered!");
                println!("🚀 Bot is ready!");
                presence::on_ready(ctx, &data).await;

                // Background GitHub poller (commits, issues). Default: every 5 minutes.
                let github_service = Arc::clone(&data.github);
                let github_http = ctx.http.clone();
                let poll_interval_secs = std::env::var("GITHUB_POLL_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(300)
                    .max(300);
                tracing::info!(
                    "GitHub poll interval set to {} seconds",
                    poll_interval_secs
                );
                tokio::spawn(async move {
                    use std::time::Duration as StdDuration;
                    use tokio::time::sleep;

                    let poll_interval = StdDuration::from_secs(poll_interval_secs);
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

                        sleep(poll_interval).await;
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
                        tracing::info!("Daily booster sweep started");

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
