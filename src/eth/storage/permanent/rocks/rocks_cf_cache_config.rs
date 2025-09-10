//! RocksDB Column Family cache configuration.

use clap::Parser;
use display_json::DebugAsJson;
use serde::Serialize;

use crate::utils::GIGABYTE;

const DEFAULT_ACCOUNTS_CACHE: usize = 10 * GIGABYTE;
const DEFAULT_ACCOUNTS_HISTORY_CACHE: usize = 5 * GIGABYTE;
const DEFAULT_ACCOUNT_SLOTS_CACHE: usize = 40 * GIGABYTE;
const DEFAULT_ACCOUNT_SLOTS_HISTORY_CACHE: usize = 20 * GIGABYTE;
const DEFAULT_TRANSACTIONS_CACHE: usize = 20 * GIGABYTE;
const DEFAULT_BLOCKS_BY_HASH_CACHE: usize = 2 * GIGABYTE;
const DEFAULT_BLOCKS_BY_NUMBER_CACHE: usize = 20 * GIGABYTE;
const DEFAULT_BLOCK_CHANGES_CACHE: usize = 2 * GIGABYTE;

/// Configuration for individual RocksDB Column Family caches.
///
/// This replaces the previous cache_size_multiplier approach with
/// individual cache size settings for each Column Family.
#[derive(DebugAsJson, Clone, Parser, Serialize)]
pub struct RocksCfCacheConfig {
    /// Cache size in bytes for the 'accounts' column family.
    #[arg(
        long = "rocks-cf-cache-accounts",
        env = "ROCKS_CF_CACHE_ACCOUNTS",
        default_value_t = DEFAULT_ACCOUNTS_CACHE
    )]
    pub accounts: usize,

    /// Cache size in bytes for the 'accounts_history' column family.
    #[arg(
        long = "rocks-cf-cache-accounts-history",
        env = "ROCKS_CF_CACHE_ACCOUNTS_HISTORY",
        default_value_t = DEFAULT_ACCOUNTS_HISTORY_CACHE
    )]
    pub accounts_history: usize,

    /// Cache size in bytes for the 'account_slots' column family.
    #[arg(
        long = "rocks-cf-cache-account-slots",
        env = "ROCKS_CF_CACHE_ACCOUNT_SLOTS",
        default_value_t = DEFAULT_ACCOUNT_SLOTS_CACHE
    )]
    pub account_slots: usize,

    /// Cache size in bytes for the 'account_slots_history' column family.
    #[arg(
        long = "rocks-cf-cache-account-slots-history",
        env = "ROCKS_CF_CACHE_ACCOUNT_SLOTS_HISTORY",
        default_value_t = DEFAULT_ACCOUNT_SLOTS_HISTORY_CACHE
    )]
    pub account_slots_history: usize,

    /// Cache size in bytes for the 'transactions' column family.
    #[arg(
        long = "rocks-cf-cache-transactions",
        env = "ROCKS_CF_CACHE_TRANSACTIONS",
        default_value_t = DEFAULT_TRANSACTIONS_CACHE
    )]
    pub transactions: usize,

    /// Cache size in bytes for the 'blocks_by_number' column family.
    #[arg(
        long = "rocks-cf-cache-blocks-by-number",
        env = "ROCKS_CF_CACHE_BLOCKS_BY_NUMBER",
        default_value_t = DEFAULT_BLOCKS_BY_NUMBER_CACHE
    )]
    pub blocks_by_number: usize,

    /// Cache size in bytes for the 'blocks_by_hash' column family.
    #[arg(
        long = "rocks-cf-cache-blocks-by-hash",
        env = "ROCKS_CF_CACHE_BLOCKS_BY_HASH",
        default_value_t = DEFAULT_BLOCKS_BY_HASH_CACHE
    )]
    pub blocks_by_hash: usize,

    /// Cache size in bytes for the 'block_changes' column family.
    #[arg(
        long = "rocks-cf-cache-block-changes",
        env = "ROCKS_CF_CACHE_BLOCK_CHANGES",
        default_value_t = DEFAULT_BLOCK_CHANGES_CACHE
    )]
    pub block_changes: usize,
}

impl Default for RocksCfCacheConfig {
    fn default() -> Self {
        Self {
            accounts: DEFAULT_ACCOUNTS_CACHE,                           // 10GB
            accounts_history: DEFAULT_ACCOUNTS_HISTORY_CACHE,           // 2GB
            account_slots: DEFAULT_ACCOUNT_SLOTS_CACHE,                 // 30GB
            account_slots_history: DEFAULT_ACCOUNT_SLOTS_HISTORY_CACHE, // 2GB
            transactions: DEFAULT_TRANSACTIONS_CACHE,                   // 2GB
            blocks_by_number: DEFAULT_BLOCKS_BY_NUMBER_CACHE,           // 2GB
            blocks_by_hash: DEFAULT_BLOCKS_BY_HASH_CACHE,               // 2GB
            block_changes: DEFAULT_BLOCK_CHANGES_CACHE,                 // 2GB
        }
    }
}

impl RocksCfCacheConfig {
    pub fn default_with_multiplier(multiplier: f64) -> Self {
        Self::default().with_multiplier(multiplier)
    }

    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.accounts = (self.accounts as f64 * multiplier) as usize;
        self.accounts_history = (self.accounts_history as f64 * multiplier) as usize;
        self.account_slots = (self.account_slots as f64 * multiplier) as usize;
        self.account_slots_history = (self.account_slots_history as f64 * multiplier) as usize;
        self.transactions = (self.transactions as f64 * multiplier) as usize;
        self.blocks_by_number = (self.blocks_by_number as f64 * multiplier) as usize;
        self.blocks_by_hash = (self.blocks_by_hash as f64 * multiplier) as usize;
        self.block_changes = (self.block_changes as f64 * multiplier) as usize;
        self
    }
}
