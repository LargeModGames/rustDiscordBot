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

/// Claim your daily GreyCoins reward
#[poise::command(slash_command, guild_only)]
pub async fn daily(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    if user.bot {
        ctx.say("Bots don't need coins! 🤖").await?;
        return Ok(());
    }

    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    // Attempt to claim daily reward
    let result = ctx.data().economy.claim_daily(user_id, guild_id).await?;

    if let Some(claim) = result {
        // Success!
        let embed = serenity::CreateEmbed::new()
            .title("✅ Daily Reward Claimed!")
            .description(format!(
                "You received **{} GreyCoins**!",
                claim.coins_awarded
            ))
            .color(0x00FF00) // Green
            .field(
                "New Balance",
                format!("🪙 {}", format_number(claim.new_balance)),
                true,
            )
            .field(
                "Next Claim",
                format!("<t:{}:R>", claim.next_claim_time.timestamp()),
                true,
            )
            .footer(serenity::CreateEmbedFooter::new(
                "Come back tomorrow for more!",
            ));

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        // On cooldown
        let next_claim = ctx
            .data()
            .economy
            .get_next_daily_time(user_id, guild_id)
            .await?;

        if let Some(next_time) = next_claim {
            let embed = serenity::CreateEmbed::new()
                .title("⏰ Daily Reward Already Claimed")
                .description("You've already claimed your daily reward today!")
                .color(0xFFA500) // Orange
                .field(
                    "Next Claim",
                    format!("<t:{}:R>", next_time.timestamp()),
                    false,
                )
                .footer(serenity::CreateEmbedFooter::new("Check back later!"));

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        } else {
            ctx.say("You can claim your daily reward now! Try again.")
                .await?;
        }
    }

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
