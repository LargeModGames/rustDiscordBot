// Discord commands for the shop system

use crate::core::economy::{ItemId, ShopItem};
use poise::serenity_prelude as serenity;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, crate::discord::commands::leveling::Data, Error>;

/// View the shop or buy items
#[poise::command(slash_command, guild_only, subcommands("list", "buy"))]
pub async fn shop(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// View available items in the shop
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let items = ShopItem::all();

    let mut embed = serenity::CreateEmbed::new()
        .title("🛒 Shop")
        .description("Purchase items with your GreyCoins!")
        .color(0x5865F2); // Blurple

    for item in items {
        let field_value = format!(
            "{} **{}** GreyCoins\n{}\n\n💰 Use `/shop buy {}` to purchase",
            item.emoji,
            format_number(item.price),
            item.description,
            item.id.as_str()
        );
        embed = embed.field(item.name, field_value, false);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Purchase an item from the shop
#[poise::command(slash_command, guild_only)]
pub async fn buy(
    ctx: Context<'_>,
    #[description = "Item to purchase"]
    #[autocomplete = "autocomplete_items"]
    item_name: String,
) -> Result<(), Error> {
    let user = ctx.author();
    if user.bot {
        ctx.say("Bots can't buy items! 🤖").await?;
        return Ok(());
    }

    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    // Parse item ID
    let item_id =
        ItemId::from_str(&item_name).ok_or_else(|| format!("Unknown item: {}", item_name))?;

    let item = ShopItem::get(&item_id);

    // Check if user has enough coins
    let wallet = ctx.data().economy.get_wallet(user_id, guild_id).await?;
    if wallet.balance < item.price {
        let embed = serenity::CreateEmbed::new()
            .title("❌ Insufficient Funds")
            .description(format!(
                "You need **{}** GreyCoins but only have **{}**.\n\n💡 Use `/daily` to earn more coins!",
                format_number(item.price),
                format_number(wallet.balance)
            ))
            .color(0xFF0000); // Red

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    // Deduct coins
    let new_balance = ctx
        .data()
        .economy
        .deduct_coins_for_purchase(
            user_id,
            guild_id,
            item.price,
            format!("Purchased {}", item.name),
        )
        .await?;

    // Add item to inventory
    ctx.data()
        .inventory
        .add_item(user_id, guild_id, item_id.clone())
        .await?;

    // Success message
    let embed = serenity::CreateEmbed::new()
        .title("✅ Purchase Successful!")
        .description(format!(
            "{} **{}** purchased for **{}** GreyCoins!\n\n💰 New balance: **{}** GreyCoins",
            item.emoji,
            item.name,
            format_number(item.price),
            format_number(new_balance)
        ))
        .color(0x00FF00); // Green

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Show your inventory
#[poise::command(slash_command, guild_only)]
pub async fn inventory(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    let user_id = user.id.get();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command only works in servers")?
        .get();

    let items = ctx
        .data()
        .inventory
        .get_inventory(user_id, guild_id)
        .await?;

    if items.is_empty() {
        let embed = serenity::CreateEmbed::new()
            .title("🎒 Your Inventory")
            .description("Your inventory is empty!\n\n💡 Use `/shop list` to see available items.")
            .color(0xFFA500); // Orange

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    // Count items by type
    let mut item_counts = std::collections::HashMap::new();
    for item in &items {
        *item_counts.entry(&item.item_id).or_insert(0) += 1;
    }

    let mut embed = serenity::CreateEmbed::new()
        .title(format!("🎒 {}'s Inventory", user.name))
        .color(0x5865F2); // Blurple

    for (item_id, count) in item_counts {
        let shop_item = ShopItem::get(item_id);
        let field_value = format!(
            "{} **Quantity:** {}\n{}",
            shop_item.emoji, count, shop_item.description
        );
        embed = embed.field(shop_item.name, field_value, false);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Autocomplete function for item names
async fn autocomplete_items<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = String> + 'a {
    let items = ShopItem::all();
    items
        .into_iter()
        .filter(move |item| {
            item.id
                .as_str()
                .to_lowercase()
                .contains(&partial.to_lowercase())
                || item.name.to_lowercase().contains(&partial.to_lowercase())
        })
        .map(|item| item.id.as_str().to_string())
        .collect::<Vec<_>>()
        .into_iter()
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
