use crate::discord::commands::leveling::{Context, Error};
use poise::serenity_prelude as serenity;
use std::collections::HashMap;

// Category definitions with emojis and order
const CATEGORY_ORDER: &[&str] = &[
    "Quick Start",
    "Music Controls",
    "Music Tools",
    "Progress & Rewards",
    "GitHub Automation",
    "Utilities",
];

fn get_category_emoji(category: &str) -> &'static str {
    match category {
        "Quick Start" => "🚀",
        "Music Controls" => "🎵",
        "Music Tools" => "🎛️",
        "Progress & Rewards" => "📈",
        "GitHub Automation" => "📡",
        "Utilities" => "🧰",
        _ => "•",
    }
}

struct CommandMetadata {
    category: &'static str,
    priority: i32,
    description: Option<&'static str>,
    note: Option<&'static str>,
}

fn get_command_metadata(name: &str) -> CommandMetadata {
    match name {
        "info" => CommandMetadata {
            category: "Quick Start",
            priority: 120,
            description: Some(
                "Post studio background details and application steps for newcomers.",
            ),
            note: None,
        },
        "play" => CommandMetadata {
            category: "Quick Start",
            priority: 110,
            description: Some("Play a song or playlist with autocomplete support."),
            note: Some("Mirrors `!p` but with Discord's slash UI."),
        },
        "profile" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 80,
            description: Some("Open an embedded version of your Greybeard profile stats."),
            note: None,
        },
        "level" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 79,
            description: Some("Check your current level and XP."),
            note: None,
        },
        "daily" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 70,
            description: Some("Claim your daily XP and GreyCoins reward once every 24 hours."),
            note: None,
        },
        "leaderboard" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 65,
            description: Some("Display the top 10 community members by level and XP."),
            note: None,
        },
        "xpstats" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 60,
            description: Some("Show detailed XP analytics for yourself or another member."),
            note: None,
        },
        "achievements" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 55,
            description: Some("View your unlocked achievements."),
            note: None,
        },
        "next_achievement" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 50,
            description: Some("Show the closest achievement you can earn."),
            note: Some("Aliases: /nextach"),
        },
        "prestige" => CommandMetadata {
            category: "Progress & Rewards",
            priority: 45,
            description: Some("Reset your level for permanent bonuses (requires level 50)."),
            note: Some("Gain XP multipliers, daily bonuses, and coin bonuses."),
        },
        "github" => CommandMetadata {
            category: "GitHub Automation",
            priority: 60,
            description: Some("Manage GitHub repository tracking."),
            note: Some("Subcommands: track, remove, list, check"),
        },
        "timezones" => CommandMetadata {
            category: "Utilities",
            priority: 42,
            description: Some("Show the current local times for key Greybeard team locations."),
            note: None,
        },
        "serverstats" => CommandMetadata {
            category: "Utilities",
            priority: 30,
            description: Some("Check that the server stats module is responding."),
            note: None,
        },
        "logging" => CommandMetadata {
            category: "Utilities",
            priority: 20,
            description: Some("Configure logging channels."),
            note: None,
        },
        "give_xp" => CommandMetadata {
            category: "Utilities",
            priority: 0, // Low priority, admin only
            description: Some("Award XP to a user (Admin only)."),
            note: None,
        },
        _ => CommandMetadata {
            category: "Utilities",
            priority: 0,
            description: None,
            note: None,
        },
    }
}

/// Show a categorized list of commands.
#[poise::command(slash_command, prefix_command)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let mut categories: HashMap<&str, Vec<(i32, String)>> = HashMap::new();

    for command in &ctx.framework().options().commands {
        if command.hide_in_help {
            continue;
        }

        let metadata = get_command_metadata(&command.name);

        // Skip if it's a specific command we want to hide (like help itself if we wanted)
        if command.name == "help" {
            continue;
        }

        let description = metadata
            .description
            .or(command.description.as_deref())
            .or(command.help_text.as_deref())
            .unwrap_or("No description provided.");

        let mut entry = format!("• **/{0}** — {1}", command.name, description);

        if let Some(note) = metadata.note {
            entry.push_str(&format!("\n  ⤷ {}", note));
        }

        categories
            .entry(metadata.category)
            .or_default()
            .push((metadata.priority, entry));
    }

    let mut embed = serenity::CreateEmbed::new()
        .title("Greybeard Command Guide")
        .description(
            "Use slash commands with `/`. \
            Everything is organised by what you want to do, and the most helpful commands \
            sit at the top of each section.",
        )
        .color(serenity::Colour::from_rgb(88, 101, 242))
        .timestamp(serenity::Timestamp::now());

    if let Some(user) = ctx.framework().bot_id.to_user(&ctx).await.ok() {
        embed = embed.thumbnail(user.face());
    }

    // Sort categories based on defined order, then alphabetically for others
    let mut sorted_categories: Vec<_> = categories.keys().cloned().collect();
    sorted_categories.sort_by(|a, b| {
        let pos_a = CATEGORY_ORDER.iter().position(|&x| x == *a).unwrap_or(999);
        let pos_b = CATEGORY_ORDER.iter().position(|&x| x == *b).unwrap_or(999);
        pos_a.cmp(&pos_b).then(a.cmp(b))
    });

    for category in sorted_categories {
        if let Some(entries) = categories.get_mut(category) {
            // Sort by priority (descending), then name (ascending)
            entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

            let emoji = get_category_emoji(category);
            let title = format!("{} {}", emoji, category);

            let formatted_entries: Vec<String> = entries.iter().map(|(_, s)| s.clone()).collect();

            // Chunk entries to avoid hitting 1024 char limit per field
            let chunks = chunk_entries(&formatted_entries);

            for (i, chunk) in chunks.iter().enumerate() {
                let field_name = if i == 0 {
                    title.clone()
                } else {
                    format!("{} (cont.)", title)
                };

                embed = embed.field(field_name, chunk.join("\n"), false);
            }
        }
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(
        "Need a hand? Ping a moderator.",
    ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

fn chunk_entries(entries: &[String]) -> Vec<Vec<String>> {
    let mut chunks = Vec::new();
    let mut current_chunk = Vec::new();
    let mut current_length = 0;

    for entry in entries {
        let entry_len = entry.len();
        // Discord field value limit is 1024. We leave a bit of buffer.
        if current_length + entry_len + 1 > 1000 {
            chunks.push(current_chunk);
            current_chunk = Vec::new();
            current_length = 0;
        }

        current_chunk.push(entry.clone());
        current_length += entry_len + 1; // +1 for newline
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}
