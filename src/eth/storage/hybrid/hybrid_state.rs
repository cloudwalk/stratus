use core::fmt;
use std::sync::Arc;

use anyhow::Context;
use indexmap::IndexMap;
use revm::primitives::KECCAK_EMPTY;
use sqlx::types::BigDecimal;
use sqlx::FromRow;
use sqlx::Pool;
use sqlx::Postgres;
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
    pub account_slots: RocksDb<(Address, SlotIndex), SlotValue>,
    pub transactions: IndexMap<Hash, TransactionMined>,
    pub blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    pub blocks_by_hash: IndexMap<Hash, Arc<Block>>,
    pub logs: IndexMap<(Hash, Index), LogMined>,
}

impl HybridStorageState {
    pub fn new() -> Self {
        Self {
            accounts: RocksDb::<Address, AccountInfo>::new("./data/accounts.rocksdb").unwrap(),
            account_slots: RocksDb::<(Address, SlotIndex), SlotValue>::new("./data/account_slots.rocksdb").unwrap(),
            transactions: IndexMap::new(),
            blocks_by_number: IndexMap::new(),
            blocks_by_hash: IndexMap::new(),
            logs: IndexMap::new(),
        }
    }

    //XXX TODO use a fixed block_number during load, in order to avoid sync problem
    // e.g other instance moving forward and this query getting incongruous data
    pub async fn load_latest_data(&mut self, pool: &Pool<Postgres>) -> anyhow::Result<()> {
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
    pub async fn update_state_with_execution_changes(&mut self, changes: &[ExecutionAccountChanges]) -> Result<(), sqlx::Error> {
        // Directly capture the fields needed by each future from `self`
        let accounts = &self.accounts;
        let account_slots = &self.account_slots;

        let changes_clone_for_accounts = changes.to_vec(); // Clone changes for accounts future
        let changes_clone_for_slots = changes.to_vec(); // Clone changes for slots future

        let account_changes_future = async move {
            let mut account_changes = Vec::new();
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
                account_changes.push((address.clone(), account_info_entry));
            }
            accounts.insert_batch(account_changes);

        };

        let slot_changes_future = async move {
            let mut slot_changes = Vec::new();
            for change in changes_clone_for_slots {
                let address = change.address.clone();
                for (slot_index, slot_change) in change.slots.clone() {
                    if let Some(slot) = slot_change.take_modified() {
                        slot_changes.push(((address.clone(), slot_index), slot.value));
                    }
                }
            }
            account_slots.insert_batch(slot_changes); // Assuming `insert_batch` is an async function
        };

        join!(account_changes_future, slot_changes_future);

        Ok(())
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
            StoragePointInTime::Past(number) => sqlx::query_as!(
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
            .context("failed to select slot")?,
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
    pub async fn to_account(&self, point_in_time: &StoragePointInTime, address: &Address) -> Account {
        match point_in_time {
            StoragePointInTime::Present => Account {
                address: address.clone(),
                nonce: self.nonce.clone(),
                balance: self.balance.clone(),
                bytecode: self.bytecode.clone(),
                code_hash: self.code_hash.clone(),
            },
            StoragePointInTime::Past(_number) => Account::default(),
        }
    }
}
