#![allow(unused_variables)]

use itertools::Itertools;
use redis::Client as RedisClient;
use redis::Commands;
use redis::Connection as RedisConnection;
use redis::RedisResult;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StoragePointInTime;
use crate::ext::from_json_str;
use crate::ext::to_json_string;
use crate::log_and_err;

type RedisOptJsonResult = RedisResult<Option<String>>;
type RedisOptUsizeResult = RedisResult<Option<usize>>;
type RedisMutationResult = RedisResult<()>;

pub struct RedisPermanentStorage {
    client: redis::Client,
}

impl RedisPermanentStorage {
    pub fn new() -> anyhow::Result<Self> {
        let client = RedisClient::open("redis://127.0.0.1/")?;
        Ok(Self { client })
    }

    fn conn(&self) -> anyhow::Result<RedisConnection> {
        match self.client.get_connection() {
            Ok(conn) => Ok(conn),
            Err(e) => log_and_err!(reason = e, "failed to get redis connection"),
        }
    }
}

impl PermanentStorage for RedisPermanentStorage {
    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        // execute command
        let mut conn = self.conn()?;

        let set: RedisMutationResult = conn.set("number::mined", number.to_string());
        match set {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to write mined number to redis"),
        }
    }

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        // execute command
        let mut conn = self.conn()?;
        let value: RedisOptUsizeResult = conn.get("number::mined");

        // parse
        match value {
            Ok(Some(value)) => Ok(value.into()),
            Ok(None) => Ok(BlockNumber::ZERO),
            Err(e) => log_and_err!(reason = e, "failed to read miner block number from redis"),
        }
    }

    fn save_block(&self, block: Block) -> anyhow::Result<()> {
        // generate block keys
        let key_number = format!("block::{}", block.number());
        let key_hash = format!("block::{}", block.hash());

        // generate values
        let block_json = to_json_string(&block);

        // blocks
        let mut redis_values = vec![
            (key_number, block_json.clone()),
            (key_hash, block_json.clone()),
            ("block::latest".to_owned(), block_json),
        ];

        // transactions
        for tx in &block.transactions {
            let tx_key = tx_key(&tx.input.hash);
            let tx_value = to_json_string(&tx);
            redis_values.push((tx_key, tx_value));
        }

        // changes
        for changes in block.compact_account_changes() {
            // account
            let mut account = Account::default();
            if let Some(nonce) = changes.nonce.take() {
                account.nonce = nonce;
            }
            if let Some(balance) = changes.balance.take() {
                account.balance = balance;
            }
            if let Some(bytecode) = changes.bytecode.take() {
                account.bytecode = bytecode;
            }
            let account_key = account_key(&account.address);
            let account_value = to_json_string(&account);
            redis_values.push((account_key, account_value));

            // slot
            for slot in changes.slots.into_values() {
                if let Some(slot) = slot.take() {
                    let slot_key = slot_key(&account.address, &slot.index);
                    let slot_value = to_json_string(&slot);
                    redis_values.push((slot_key, slot_value));
                }
            }
        }

        // execute command
        let mut conn = self.conn()?;
        let set: RedisMutationResult = conn.mset(&redis_values);
        match set {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to write block to redis"),
        }
    }

    fn read_block(&self, block_filter: &BlockFilter) -> anyhow::Result<Option<Block>> {
        let block_key = match block_filter {
            BlockFilter::Latest | BlockFilter::Pending => "block::latest".to_owned(),
            BlockFilter::Earliest => "block::earliest".to_owned(),
            BlockFilter::Hash(hash) => format!("block::{}", hash),
            BlockFilter::Number(number) => format!("block::{}", number),
        };

        // execute command
        let mut conn = self.conn()?;
        let redis_block: RedisOptJsonResult = conn.get(block_key);

        // parse
        match redis_block {
            Ok(Some(json)) => Ok(from_json_str(&json)),
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to retrieve block from redis"),
        }
    }

    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let tx_key = tx_key(hash);

        // execute command
        let mut conn = self.conn()?;
        let redis_transaction: RedisOptJsonResult = conn.get(tx_key);

        // parse
        match redis_transaction {
            Ok(Some(json)) => Ok(from_json_str(&json)),
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to retrieve transaction from redis"),
        }
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        todo!()
    }

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        if accounts.is_empty() {
            return Ok(());
        }

        let redis_accounts = accounts
            .into_iter()
            .map(|acc| {
                let account_key = account_key(&acc.address);
                let account_value = to_json_string(&acc);
                (account_key, account_value)
            })
            .collect_vec();

        // execute command
        let mut conn = self.conn()?;
        let set: RedisMutationResult = conn.mset(&redis_accounts);

        match set {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to write accounts to redis"),
        }
    }

    fn read_account(&self, address: &Address, point_in_time: &crate::eth::storage::StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let account_key = account_key(address);

        // execute and parse
        let mut conn = self.conn()?;
        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                let redis_account: RedisOptJsonResult = conn.get(account_key);
                match redis_account {
                    Ok(Some(json)) => Ok(Some(from_json_str(&json))),
                    Ok(None) => Ok(None),
                    Err(e) => log_and_err!(reason = e, "failed to read account from redis"),
                }
            }
            StoragePointInTime::MinedPast(number) => todo!(),
        }
    }

    fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &crate::eth::storage::StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let slot_key = slot_key(address, index);

        // execute and parse
        let mut conn = self.conn()?;
        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                let redis_slot: RedisOptJsonResult = conn.get(slot_key);
                match redis_slot {
                    Ok(Some(json)) => Ok(Some(from_json_str(&json))),
                    Ok(None) => Ok(None),
                    Err(e) => log_and_err!(reason = e, "failed to read slot from redis"),
                }
            }
            StoragePointInTime::MinedPast(number) => todo!(),
        }
    }

    fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()> {
        let mut conn = self.conn()?;
        let flush: RedisMutationResult = redis::cmd("FLUSHDB").exec(&mut conn);
        match flush {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to clear all redis keys"),
        }
    }
}

// -----------------------------------------------------------------------------
// Keys helpers
// -----------------------------------------------------------------------------

/// Generates a key for accessing an account.
fn account_key(address: &Address) -> String {
    format!("account::{}", address)
}

/// Generates a key for accessing a slot.
fn slot_key(address: &Address, index: &SlotIndex) -> String {
    format!("slot::{}::{}", address, index)
}

/// Generates a key for accessing a transaction.
fn tx_key(hash: &Hash) -> String {
    format!("tx::{}", hash)
}
