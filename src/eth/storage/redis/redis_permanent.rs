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

type RedisVecOptString = RedisResult<Vec<Option<String>>>;
type RedisVecString = RedisResult<Vec<String>>;
type RedisOptString = RedisResult<Option<String>>;
type RedisOptUsize = RedisResult<Option<usize>>;
type RedisVoid = RedisResult<()>;

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
        let set: RedisVoid = conn.set("number::mined", number.to_string());

        // parse
        match set {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to write mined number to redis"),
        }
    }

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        // execute command
        let mut conn = self.conn()?;
        let value: RedisOptUsize = conn.get("number::mined");

        // parse
        match value {
            Ok(Some(value)) => Ok(value.into()),
            Ok(None) => Ok(BlockNumber::ZERO),
            Err(e) => log_and_err!(reason = e, "failed to read miner block number from redis"),
        }
    }

    fn save_block(&self, block: Block) -> anyhow::Result<()> {
        // generate block keys
        let key_block_number = key_block_by_number(block.number());
        let key_block_hash = key_block_by_hash(&block.hash());

        // generate values
        let block_json = to_json_string(&block);

        // blocks
        let mut mset_values = vec![
            (key_block_number, block_json.clone()),
            (key_block_hash, block_json.clone()),
            ("block::latest".to_owned(), block_json),
        ];
        let mut zadd_values = vec![];

        // transactions
        for tx in &block.transactions {
            let tx_key = key_tx(&tx.input.hash);
            let tx_value = to_json_string(&tx);
            mset_values.push((tx_key, tx_value));
        }

        // changes
        for changes in block.compact_account_changes() {
            // account
            let mut account = Account {
                address: changes.address,
                ..Account::default()
            };
            if let Some(nonce) = changes.nonce.take() {
                account.nonce = nonce;
            }
            if let Some(balance) = changes.balance.take() {
                account.balance = balance;
            }
            if let Some(bytecode) = changes.bytecode.take() {
                account.bytecode = bytecode;
            }

            let account_value = to_json_string(&account);
            mset_values.push((key_account(&account.address), account_value.clone()));
            zadd_values.push((key_account_history(&account.address), account_value, block.number().as_u64()));

            // slot
            for slot in changes.slots.into_values() {
                if let Some(slot) = slot.take() {
                    let slot_value = to_json_string(&slot);
                    mset_values.push((key_slot(&account.address, &slot.index), slot_value.clone()));
                    zadd_values.push((key_slot_history(&account.address, &slot.index), slot_value, block.number().as_u64()));
                }
            }
        }

        // execute mset command
        let mut conn = self.conn()?;
        let set: RedisVoid = conn.mset(&mset_values);
        if let Err(e) = set {
            return log_and_err!(reason = e, "failed to write block mset to redis");
        }

        // execute zadd commands
        for (key, value, score) in zadd_values {
            let zadd: RedisVoid = conn.zadd(key, value, score);
            if let Err(e) = zadd {
                return log_and_err!(reason = e, "failed to write block zadd to redis");
            }
        }

        Ok(())
    }

    fn read_block(&self, block_filter: &BlockFilter) -> anyhow::Result<Option<Block>> {
        // prepare keys
        let block_key = match block_filter {
            BlockFilter::Latest | BlockFilter::Pending => "block::latest".to_owned(),
            BlockFilter::Earliest => "block::earliest".to_owned(),
            BlockFilter::Hash(hash) => key_block_by_hash(hash),
            BlockFilter::Number(number) => key_block_by_number(*number),
        };

        // execute command
        let mut conn = self.conn()?;
        let redis_block: RedisOptString = conn.get(block_key);

        // parse
        match redis_block {
            Ok(Some(json)) => Ok(from_json_str(&json)),
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to read block from redis"),
        }
    }

    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        // prepare keys
        let tx_key = key_tx(hash);

        // execute command
        let mut conn = self.conn()?;
        let redis_transaction: RedisOptString = conn.get(tx_key);

        // parse
        match redis_transaction {
            Ok(Some(json)) => Ok(from_json_str(&json)),
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to read transaction from redis"),
        }
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        // prepare keys
        let from_block = filter.from_block.as_u64();
        let to_block = match filter.to_block {
            Some(number) => number.as_u64(),
            None => self.read_mined_block_number()?.as_u64(),
        };
        let block_keys = (from_block..=to_block).map(key_block_by_number).collect_vec();

        // exit if no keys
        if block_keys.is_empty() {
            return Ok(vec![]);
        }

        // execute command
        let mut conn = self.conn()?;
        let blocks: RedisVecOptString = conn.mget(block_keys);

        // parse
        let blocks: Vec<Block> = match blocks {
            Ok(vec_json) => vec_json.into_iter().flatten().map(|json| from_json_str(&json)).collect_vec(),
            Err(e) => return log_and_err!(reason = e, "failed to read logs from redis"),
        };

        // filter
        let logs = blocks
            .into_iter()
            .flat_map(|b| b.transactions)
            .flat_map(|t| t.logs)
            .filter(|log| filter.matches(log))
            .collect_vec();

        Ok(logs)
    }

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        // exit if no accounts
        if accounts.is_empty() {
            return Ok(());
        }

        // prepare values
        let redis_accounts = accounts
            .into_iter()
            .map(|acc| {
                let account_key = key_account(&acc.address);
                let account_value = to_json_string(&acc);
                (account_key, account_value)
            })
            .collect_vec();

        // execute command
        let mut conn = self.conn()?;
        let set: RedisVoid = conn.mset(&redis_accounts);

        // parse
        match set {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to write accounts to redis"),
        }
    }

    fn read_account(&self, address: &Address, point_in_time: &crate::eth::storage::StoragePointInTime) -> anyhow::Result<Option<Account>> {
        // prepare keys

        // execute and parse
        let mut conn = self.conn()?;
        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                // prepare key
                let account_key = key_account(address);

                // execute
                let redis_account: RedisOptString = conn.get(account_key);

                // parse
                match redis_account {
                    Ok(Some(json)) => Ok(Some(from_json_str(&json))),
                    Ok(None) => Ok(None),
                    Err(e) => log_and_err!(reason = e, "failed to read account from redis current value"),
                }
            }
            StoragePointInTime::MinedPast(number) => {
                // prepare key
                let account_key = key_account_history(address);

                // execute
                let mut cmd = redis::cmd("ZRANGE");
                cmd.arg(account_key)
                    .arg(number.as_u64())
                    .arg(0)
                    .arg("BYSCORE")
                    .arg("REV")
                    .arg("LIMIT")
                    .arg(0)
                    .arg(1);
                let redis_account: RedisVecString = cmd.query(&mut conn);

                // parse
                match redis_account {
                    Ok(vec_json) => match vec_json.first() {
                        Some(json) => Ok(Some(from_json_str(json))),
                        None => Ok(None),
                    },
                    Err(e) => log_and_err!(reason = e, "failed to read account from redis historical value"),
                }
            }
        }
    }

    fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        // execute command and parse
        let mut conn = self.conn()?;
        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                // prepare key
                let slot_key = key_slot(address, index);

                // execute
                let redis_slot: RedisOptString = conn.get(slot_key);

                // parse
                match redis_slot {
                    Ok(Some(json)) => Ok(Some(from_json_str(&json))),
                    Ok(None) => Ok(None),
                    Err(e) => log_and_err!(reason = e, "failed to read slot from redis current value"),
                }
            }
            StoragePointInTime::MinedPast(number) => {
                // prepare key
                let slot_key = key_slot_history(address, index);

                // execute
                let mut cmd = redis::cmd("ZRANGE");
                cmd.arg(slot_key)
                    .arg(number.as_u64())
                    .arg(0)
                    .arg("BYSCORE")
                    .arg("REV")
                    .arg("LIMIT")
                    .arg(0)
                    .arg(1);
                let redis_account: RedisVecString = cmd.query(&mut conn);

                // parse
                match redis_account {
                    Ok(vec_json) => match vec_json.first() {
                        Some(json) => Ok(Some(from_json_str(json))),
                        None => Ok(None)
                    },
                    Err(e) => log_and_err!(reason = e, "failed to read account from redis historical value"),
                }
            }
        }
    }

    #[cfg(feature = "dev")]
    fn reset(&self) -> anyhow::Result<()> {
        let mut conn = self.conn()?;
        let flush: RedisVoid = redis::cmd("FLUSHDB").exec(&mut conn);
        match flush {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to clear all redis keys"),
        }
    }
}

// -----------------------------------------------------------------------------
// Keys helpers
// -----------------------------------------------------------------------------

/// Generates a key for accessing a block by number.
fn key_block_by_number(number: impl Into<u64>) -> String {
    format!("block::number::{}", number.into())
}

/// Generates a key for accessing a block by hash.
fn key_block_by_hash(hash: &Hash) -> String {
    format!("block::hash::{}", hash)
}

/// Generates a key for accessing an account.
fn key_account(address: &Address) -> String {
    format!("account::{}", address)
}

/// Generates a key for accessing an account history.
fn key_account_history(address: &Address) -> String {
    format!("account_history::{}", address)
}

/// Generates a key for accessing a slot.
fn key_slot(address: &Address, index: &SlotIndex) -> String {
    format!("slot::{}::{}", address, index)
}

/// Generates a key for accessing a slot history.
fn key_slot_history(address: &Address, index: &SlotIndex) -> String {
    format!("slot_history::{}::{}", address, index)
}

/// Generates a key for accessing a transaction.
fn key_tx(hash: &Hash) -> String {
    format!("tx::{}", hash)
}
