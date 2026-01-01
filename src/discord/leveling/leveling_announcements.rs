use crate::core::leveling::LevelUpEvent;
use crate::discord::Data;
use poise::serenity_prelude::{self as serenity, builder::CreateMessage};
use rand::seq::SliceRandom;

/// Send a Greybeard-style level-up embed to the leveling announcements channel.
pub async fn send_level_up_embed(
    ctx: &serenity::Context,
    message: &serenity::Message,
    data: &Data,
    level_up: &LevelUpEvent,
) -> Result<(), serenity::Error> {
    let announcement_channel_id = serenity::ChannelId::from(1456341010262266114u64);
    let leveling = &data.leveling;
    let previous_threshold = leveling.xp_for_level(level_up.new_level);
    let next_threshold = leveling.xp_for_next_level(level_up.new_level);
    let level_span = next_threshold.saturating_sub(previous_threshold).max(1);
    let xp_in_level = level_up
        .total_xp
        .saturating_sub(previous_threshold)
        .min(level_span);
    let progress = xp_in_level as f64 / level_span as f64;

    let embed = serenity::CreateEmbed::new()
        .title("Level Up!")
        .description(format!(
            "<@{}> reached level {}!",
            level_up.user_id, level_up.new_level
        ))
        .color(level_color(level_up.new_level))
        .field("Total XP", level_up.total_xp.to_string(), true)
        .field(
            "Progress",
            format!(
                "{}/{} XP\n{}",
                xp_in_level,
                level_span,
                build_progress_bar(progress, 18)
            ),
            false,
        )
        .footer(serenity::CreateEmbedFooter::new(random_flavor_line()));

    announcement_channel_id
        .send_message(ctx, CreateMessage::new().embed(embed))
        .await
        .map(|_| ())
}

fn level_color(level: u32) -> serenity::Colour {
    if level >= 50 {
        serenity::Colour::DARK_PURPLE
    } else if level >= 25 {
        serenity::Colour::ORANGE
    } else if level >= 10 {
        serenity::Colour::GOLD
    } else if level >= 5 {
        serenity::Colour::BLURPLE
    } else {
        serenity::Colour::LIGHT_GREY
    }
}

fn build_progress_bar(progress: f64, length: usize) -> String {
    let clamped = progress.clamp(0.0, 1.0);
    let mut filled = (clamped * length as f64).round() as usize;
    if clamped > 0.0 && filled == 0 {
        filled = 1;
    }
    filled = filled.min(length);
    let filled_char = "▰";
    let empty_char = "▱";
    let bar = filled_char.repeat(filled) + &empty_char.repeat(length - filled);
    format!("{} ({}%)", bar, (clamped * 100.0).round() as u32)
}

fn random_flavor_line() -> &'static str {
    const FLAVOR_LINES: [&str; 4] = [
        "Keep the streak going!",
        "Your grind is paying off.",
        "Another level, another flex.",
        "That XP bar never stood a chance.",
    ];

    FLAVOR_LINES
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or(FLAVOR_LINES[0])
}
