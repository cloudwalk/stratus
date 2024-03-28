use core::fmt;

use anyhow::Context;
use itertools::Itertools;
use revm::primitives::KECCAK_EMPTY;
use sqlx::types::BigDecimal;
use sqlx::types::Json;
use sqlx::FromRow;
use sqlx::Pool;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use sqlx::Row;
use tokio::join;

use super::rocks_db::RocksDb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AccountInfo {
    pub balance: Wei,
    pub nonce: Nonce,
    pub bytecode: Option<Bytes>,
    pub code_hash: CodeHash,
}

#[derive(FromRow)]
struct AccountRow {
    address: Vec<u8>,
    nonce: Option<BigDecimal>,
    balance: Option<BigDecimal>,
    bytecode: Option<Vec<u8>>,
    code_hash: CodeHash,
}

#[derive(FromRow)]
struct SlotRow {
    account_address: Vec<u8>,
    slot_index: SlotIndex,
    value: Option<Vec<u8>>,
}

pub struct HybridStorageState {
    pub accounts: RocksDb<Address, AccountInfo>,
    pub accounts_history: RocksDb<(Address, BlockNumber), AccountInfo>,
    pub account_slots: RocksDb<(Address, SlotIndex), SlotValue>,
    pub account_slots_history: RocksDb<(Address, SlotIndex, BlockNumber), SlotValue>,
    pub transactions: RocksDb<Hash, TransactionMined>,
    pub blocks_by_number: RocksDb<BlockNumber, Block>,
    pub blocks_by_hash: RocksDb<Hash, Block>,
    pub logs: RocksDb<(Hash, Index), LogMined>,
}

impl HybridStorageState {
    pub fn new() -> Self {
        Self {
            accounts: RocksDb::new("./data/accounts.rocksdb").unwrap(),
            accounts_history: RocksDb::new("./data/accounts_history.rocksdb").unwrap(),
            account_slots: RocksDb::new("./data/account_slots.rocksdb").unwrap(),
            account_slots_history: RocksDb::new("./data/account_slots_history.rocksdb").unwrap(),
            transactions: RocksDb::new("./data/transactions.rocksdb").unwrap(),
            blocks_by_number: RocksDb::new("./data/blocks_by_number.rocksdb").unwrap(),
            blocks_by_hash: RocksDb::new("./data/blocks_by_hash.rocksdb").unwrap(),
            logs: RocksDb::new("./data/logs.rocksdb").unwrap(),
        }
    }

    //XXX TODO use a fixed block_number during load, in order to avoid sync problem
    // e.g other instance moving forward and this query getting incongruous data
    pub async fn load_latest_data(&self, pool: &Pool<Postgres>) -> anyhow::Result<()> {
        let account_rows = sqlx::query_as!(
            AccountRow,
            "
            SELECT DISTINCT ON (address)
                address,
                nonce,
                balance,
                bytecode,
                code_hash
            FROM
                neo_accounts
            ORDER BY
                address,
                block_number DESC
            "
        )
        .fetch_all(pool)
        .await?;

        for account_row in account_rows {
            let addr: Address = account_row.address.try_into().unwrap_or_default(); //XXX add alert
            self.accounts.insert(
                addr,
                AccountInfo {
                    balance: account_row.balance.map(|b| b.try_into().unwrap_or_default()).unwrap_or_default(),
                    nonce: account_row.nonce.map(|n| n.try_into().unwrap_or_default()).unwrap_or_default(),
                    bytecode: account_row.bytecode.map(Bytes::from),
                    code_hash: account_row.code_hash,
                },
            );
        }

        // Load slots
        let slot_rows = sqlx::query_as!(
            SlotRow,
            "
            SELECT DISTINCT ON (account_address, slot_index)
                account_address,
                slot_index,
                value
            FROM
                neo_account_slots
            ORDER BY
                account_address,
                slot_index,
                block_number DESC
            "
        )
        .fetch_all(pool)
        .await?;

        for slot_row in slot_rows {
            let addr: Address = slot_row.account_address.try_into().unwrap_or_default(); //XXX add alert
            self.account_slots
                .insert((addr, slot_row.slot_index), slot_row.value.unwrap_or_default().into());
        }

        Ok(())
    }

    /// Updates the in-memory state with changes from transaction execution
    pub async fn update_state_with_execution_changes(&self, changes: &[ExecutionAccountChanges], block_number: BlockNumber) -> Result<(), sqlx::Error> {
        // Directly capture the fields needed by each future from `self`
        let accounts = &self.accounts;
        let accounts_history = &self.accounts_history;
        let account_slots = &self.account_slots;
        let account_slots_history = &self.account_slots_history;

        let changes_clone_for_accounts = changes.to_vec(); // Clone changes for accounts future
        let changes_clone_for_slots = changes.to_vec(); // Clone changes for slots future

        let account_changes_future = async move {
            let mut account_changes = Vec::new();
            let mut account_history_changes = Vec::new();

            for change in changes_clone_for_accounts {
                let address = change.address.clone();
                let mut account_info_entry = accounts.entry_or_insert_with(address.clone(), || AccountInfo {
                    balance: Wei::ZERO, // Initialize with default values
                    nonce: Nonce::ZERO,
                    bytecode: None,
                    code_hash: KECCAK_EMPTY.into(),
                });
                if let Some(nonce) = change.nonce.clone().take_modified() {
                    account_info_entry.nonce = nonce;
                }
                if let Some(balance) = change.balance.clone().take_modified() {
                    account_info_entry.balance = balance;
                }
                if let Some(bytecode) = change.bytecode.clone().take_modified() {
                    account_info_entry.bytecode = bytecode;
                }
                account_changes.push((address.clone(), account_info_entry.clone()));
                account_history_changes.push(((address.clone(), block_number), account_info_entry));
            }
            accounts.insert_batch(account_changes);
            accounts_history.insert_batch(account_history_changes);
        };

        let slot_changes_future = async move {
            let mut slot_changes = Vec::new();
            let mut slot_history_changes = Vec::new();

            for change in changes_clone_for_slots {
                let address = change.address.clone();
                for (slot_index, slot_change) in change.slots.clone() {
                    if let Some(slot) = slot_change.take_modified() {
                        slot_changes.push(((address.clone(), slot_index.clone()), slot.value.clone()));
                        slot_history_changes.push(((address.clone(), slot_index, block_number), slot.value));
                    }
                }
            }
            account_slots.insert_batch(slot_changes); // Assuming `insert_batch` is an async function
            account_slots_history.insert_batch(slot_history_changes);
        };

        join!(account_changes_future, slot_changes_future);

        Ok(())
    }

    pub async fn read_logs(&self, filter: &LogFilter, pool: &Pool<Postgres>) -> anyhow::Result<Vec<LogMined>> {
        let logs = self
            .logs
            .iter_start()
            .skip_while(|(_, log)| log.block_number < filter.from_block)
            .take_while(|(_, log)| match filter.to_block {
                Some(to_block) => log.block_number <= to_block,
                None => true,
            })
            .filter_map(|(_, log)| if filter.matches(&log) { Some(log) } else { None });

        let log_query_builder = &mut QueryBuilder::new(
            r#"
                SELECT log_data
                FROM neo_logs
            "#,
        );
        log_query_builder.push(" WHERE block_number >= ").push_bind(filter.from_block);

        // verifies if to_block exists
        if let Some(block_number) = filter.to_block {
            log_query_builder.push(" AND block_number <= ").push_bind(block_number);
        }

        for address in filter.addresses.iter() {
            log_query_builder.push(" AND address = ").push_bind(address);
        }

        let log_query = log_query_builder.build();

        let query_result = log_query.fetch_all(pool).await?;

        let pg_logs = query_result
            .into_iter()
            .map(|row| {
                let json: Json<LogMined> = row.get("log_data");
                json.0
            })
            .filter(|log| filter.matches(log))
            .chain(logs) // we chain the iterators because it might be the case that some logs are yet to be written to pg
            .unique_by(|log| (log.block_number, log.log_index, log.transaction_hash.clone()))
            .collect();

        Ok(pg_logs)
    }

    pub async fn get_slot_at_point(
        &self,
        address: &Address,
        slot_index: &SlotIndex,
        point_in_time: &StoragePointInTime,
        pool: &Pool<Postgres>,
    ) -> anyhow::Result<Option<Slot>> {
        let slot = match point_in_time {
            StoragePointInTime::Present => self.account_slots.get(&(address.clone(), slot_index.clone())).map(|account_slot_value| Slot {
                index: slot_index.clone(),
                value: account_slot_value.clone(),
            }),
            StoragePointInTime::Past(number) => {
                if let Some(((addr, index, _), value)) = self
                    .account_slots_history
                    .iter_from((address.clone(), slot_index.clone(), *number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if slot_index == &index && address == &addr {
                        return Ok(Some(Slot { index, value }));
                    }
                }
                {
                    sqlx::query_as!(
                        Slot,
                        r#"
                    SELECT
                    slot_index as "index: _",
                    value as "value: _"
                    FROM neo_account_slots
                    WHERE account_address = $1
                    AND slot_index = $2
                    AND block_number = (SELECT MAX(block_number)
                                            FROM historical_slots
                                            WHERE account_address = $1
                                            AND idx = $2
                                            AND block_number <= $3)
                "#,
                        address as _,
                        slot_index as _,
                        number as _
                    )
                    .fetch_optional(pool)
                    .await
                    .context("failed to select slot")?
                }
            }
        };
        Ok(slot)
    }
}

impl fmt::Debug for HybridStorageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDb").field("db", &"Arc<DB>").finish()
    }
}

impl AccountInfo {
    pub async fn to_account(&self, address: &Address) -> Account {
        Account {
            address: address.clone(),
            nonce: self.nonce.clone(),
            balance: self.balance.clone(),
            bytecode: self.bytecode.clone(),
            code_hash: self.code_hash.clone(),
        }
    }
}
