use crate::discord::{Context, Error};
use poise::serenity_prelude as serenity;

/// Display the current time across the core team timezones.
#[poise::command(slash_command, aliases("tz", "times"))]
pub async fn timezones(ctx: Context<'_>) -> Result<(), Error> {
    let timezones = ctx.data().timezones.get_team_timezones();

    let mut embed = serenity::CreateEmbed::new()
        .title("🌍 Team Timezones")
        .description(
            "Quick snapshot of local times for our distributed crew. \
             Use this before scheduling meetings or releases.",
        )
        .color(0x5865F2) // Blurple
        .timestamp(serenity::Timestamp::now());

    // If bot user is available, set thumbnail
    if let Some(bot_user) = ctx.framework().bot_id.to_user(&ctx).await.ok() {
        embed = embed.thumbnail(bot_user.face());
    }

    for (tz_def, display) in timezones {
        let value = format!(
            "**{}** ({})\n{}\n<t:{}:R>\n_{}_",
            display.twelve_hour,
            display.twenty_four_hour,
            display.date_fragment,
            display.relative_timestamp,
            tz_def.note
        );
        embed = embed.field(tz_def.label, value, false);
    }

    embed = embed.footer(serenity::CreateEmbedFooter::new(
        "All times update on demand. Powered by chrono-tz.",
    ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
