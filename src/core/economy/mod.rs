// Economy module - domain logic for GreyCoins currency system

mod economy_service;

pub use economy_service::{CoinStore, EconomyError, EconomyService, Transaction, Wallet};
