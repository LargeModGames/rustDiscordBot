// Economy module - domain logic for GreyCoins currency system

mod economy_service;
pub mod inventory_service;
pub mod item_definitions;

pub use economy_service::{CoinStore, EconomyError, EconomyService, Transaction, Wallet};
pub use inventory_service::{InventoryItem, InventoryService, InventoryStore};
pub use item_definitions::{ItemId, ShopItem};
