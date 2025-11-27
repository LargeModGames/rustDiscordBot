// Discord commands for the economy system
//
// Following the same pattern as leveling commands:
// 1. Extract primitive data from Discord types
// 2. Call core service
// 3. Format the response

use poise::serenity_prelude as serenity;

// Re-use the same type aliases from leveling commands
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, crate::discord::commands::leveling::Data, Error>;

/// Check your GreyCoins balance
#[poise::command(slash_command, guild_only)]
pub async fn balance(
    ctx: Context<'_>,
    #[description = "User to check balance for (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_id = target_user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    if target_user.bot {
        ctx.say("Bots don't have wallets! 🤖").await?;
        return Ok(());
    }

    // Get wallet information
    let wallet = ctx.data().economy.get_wallet(user_id, guild_id).await?;

    // Get recent transactions
    let transactions = ctx
        .data()
        .economy
        .get_recent_transactions(user_id, guild_id, 5)
        .await?;

    // Format transactions
    let transaction_text = if transactions.is_empty() {
        "No transactions yet".to_string()
    } else {
        transactions
            .iter()
            .map(|t| {
                let sign = if t.amount >= 0 { "+" } else { "" };
                format!("{}{} 🪙 — {}", sign, format_number(t.amount), t.reason)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let embed = serenity::CreateEmbed::new()
        .title(format!("💰 {}'s Wallet", target_user.name))
        .color(0xFFD700) // Gold color
        .thumbnail(target_user.face())
        .field(
            "Balance",
            format!("🪙 **{} GreyCoins**", format_number(wallet.balance)),
            true,
        )
        .field(
            "Total Earned",
            format!("🪙 {}", format_number(wallet.total_earned)),
            true,
        )
        .field("Recent Transactions", transaction_text, false)
        .footer(serenity::CreateEmbedFooter::new(
            "Use /daily to claim your daily reward!",
        ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Claim your daily rewards (XP and GreyCoins)
#[poise::command(slash_command, guild_only)]
pub async fn daily(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    if user.bot {
        ctx.say("Bots don't need daily rewards! 🤖").await?;
        return Ok(());
    }

    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    // Detect booster status for XP bonus
    let boosted = ctx
        .serenity_context()
        .cache
        .guild(serenity::GuildId::from(guild_id))
        .and_then(|g| g.members.get(&serenity::UserId::from(user_id)).cloned())
        .and_then(|m| m.premium_since)
        .is_some();

    let member_count = ctx.guild().map(|g| g.member_count).unwrap_or(0);

    // Attempt to claim XP daily reward
    let (xp_award, levelup_opt) = ctx
        .data()
        .leveling
        .claim_daily(user_id, guild_id, boosted, member_count)
        .await?;

    // Attempt to claim GreyCoins daily reward
    let coin_result = ctx.data().economy.claim_daily(user_id, guild_id).await?;

    // Get daily goal state for display
    let daily_goal = ctx
        .data()
        .leveling
        .get_daily_goal_state(guild_id, member_count)
        .await?;
    let goal_progress = daily_goal.progress as f64 / daily_goal.target as f64;
    let progress_bar =
        build_progress_bar(goal_progress, std::cmp::min(daily_goal.target as usize, 18));

    // Get current streak
    let profile = ctx
        .data()
        .leveling
        .get_user_profile(user_id, guild_id)
        .await?;

    // Both are on cooldown
    if xp_award == 0 && coin_result.is_none() {
        let now = chrono::Utc::now();
        let next_xp_claim = profile.last_daily.map(|d| d + chrono::Duration::days(1));
        let next_coin_claim = ctx
            .data()
            .economy
            .get_next_daily_time(user_id, guild_id)
            .await?;

        // Find the earliest next claim time
        let next_claim = match (next_xp_claim, next_coin_claim) {
            (Some(xp), Some(coin)) => Some(std::cmp::min(xp, coin)),
            (Some(xp), None) => Some(xp),
            (None, Some(coin)) => Some(coin),
            (None, None) => None,
        };

        let time_str = if let Some(next_time) = next_claim {
            let time_remaining = next_time.signed_duration_since(now);
            if time_remaining.num_seconds() <= 0 {
                "Ready soon".to_string()
            } else if time_remaining.num_minutes() < 60 {
                format!(
                    "{}m {}s",
                    time_remaining.num_minutes(),
                    time_remaining.num_seconds() % 60
                )
            } else if time_remaining.num_hours() < 24 {
                format!(
                    "{}h {}m",
                    time_remaining.num_hours(),
                    time_remaining.num_minutes() % 60
                )
            } else {
                format!(
                    "{}d {}h",
                    time_remaining.num_days(),
                    time_remaining.num_hours() % 24
                )
            }
        } else {
            "Unknown".to_string()
        };

        let embed = serenity::CreateEmbed::new()
            .title("⏰ Daily Reward Already Claimed")
            .description(format!(
                "You have already claimed your daily reward. Time until next claim: {}",
                time_str
            ))
            .color(0xFFA500) // Orange
            .field("Streak", format!("{} days 🔥", profile.daily_streak), true)
            .field(
                "Server Goal",
                format!(
                    "{}/{} claims\n{}",
                    daily_goal.progress, daily_goal.target, progress_bar
                ),
                false,
            );

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    // At least one reward was claimed - build success embed
    let mut description_parts = Vec::new();

    if xp_award > 0 {
        if let Some(ref level_up) = levelup_opt {
            description_parts.push(format!(
                "✨ **+{} XP** — Leveled up to **{}**!",
                xp_award, level_up.new_level
            ));
        } else {
            description_parts.push(format!("✨ **+{} XP**", xp_award));
        }
    }

    if let Some(ref claim) = coin_result {
        description_parts.push(format!(
            "🪙 **+{} GreyCoins** (Balance: {})",
            claim.coins_awarded,
            format_number(claim.new_balance)
        ));
    }

    let description = description_parts.join("\n");

    let embed = serenity::CreateEmbed::new()
        .title("✅ Daily Reward Claimed!")
        .description(description)
        .color(0x00FF00) // Green
        .field("Streak", format!("{} days 🔥", profile.daily_streak), true)
        .field(
            "Server Goal",
            format!(
                "{}/{} claims\n{}",
                daily_goal.progress, daily_goal.target, progress_bar
            ),
            false,
        )
        .footer(serenity::CreateEmbedFooter::new(
            "Come back tomorrow for more!",
        ));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Format a number with commas for readability
fn format_number(n: i64) -> String {
    let s = n.to_string();
    let negative = s.starts_with('-');
    let s = if negative { &s[1..] } else { &s };

    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }

    if negative {
        result.insert(0, '-');
    }

    result
}

/// Build a visual progress bar using Unicode characters
fn build_progress_bar(progress: f64, length: usize) -> String {
    let clamped = progress.clamp(0.0, 1.0);
    let mut filled = (clamped * length as f64).round() as usize;
    if clamped > 0.0 && filled == 0 {
        filled = 1;
    }
    if filled > length {
        filled = length;
    }
    let filled_char = "▰";
    let empty_char = "▱";
    let bar = filled_char.repeat(filled) + &empty_char.repeat(length - filled);
    format!("{} ({}%)", bar, (clamped * 100.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(-1234567), "-1,234,567");
    }
}
