use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use sqlx::types::BigDecimal;
use sqlx::FromRow;
use sqlx::Pool;
use sqlx::Postgres;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::Wei;

#[derive(Debug)]
struct SlotInfo {
    value: SlotValue,
}

#[derive(Debug)]
pub struct AccountInfo {
    balance: Wei,
    nonce: Nonce,
    bytecode: Option<Bytes>,
    slots: HashMap<SlotIndex, SlotInfo>,
}

#[derive(Debug)]
pub struct HybridHistory {
    pub hybrid_accounts_slots: HashMap<Address, AccountInfo>,
    pool: Arc<Pool<Postgres>>,
}

#[derive(FromRow)]
struct AccountRow {
    address: Vec<u8>,
    nonce: Option<BigDecimal>,
    balance: Option<BigDecimal>,
    bytecode: Option<Vec<u8>>,
}

#[derive(FromRow)]
struct SlotRow {
    account_address: Vec<u8>,
    slot_index: SlotIndex,
    value: Option<Vec<u8>>,
}

impl HybridHistory {
    pub async fn new(pool: Arc<Pool<Postgres>>) -> Result<Self, sqlx::Error> {
        // Initialize the structure
        let mut history = HybridHistory {
            hybrid_accounts_slots: HashMap::new(),
            pool,
        };

        history.load_latest_data().await?;

        Ok(history)
    }

    //XXX TODO use a fixed block_number during load, in order to avoid sync problem
    // e.g other instance moving forward and this query getting incongruous data
    async fn load_latest_data(&mut self) -> Result<(), sqlx::Error> {
        let account_rows = sqlx::query_as!(
            AccountRow,
            "
            SELECT DISTINCT ON (address)
                address,
                nonce,
                balance,
                bytecode
            FROM
                neo_accounts
            ORDER BY
                address,
                block_number DESC
            "
        )
        .fetch_all(&*self.pool)
        .await?;

        let mut accounts: HashMap<Address, AccountInfo> = HashMap::new();

        for account_row in account_rows {
            let addr: Address = account_row.address.try_into().unwrap_or_default(); //XXX add alert
            accounts.insert(
                addr,
                AccountInfo {
                    balance: account_row.balance.map(|b| b.try_into().unwrap_or_default()).unwrap_or_default(),
                    nonce: account_row.nonce.map(|n| n.try_into().unwrap_or_default()).unwrap_or_default(),
                    bytecode: account_row.bytecode.map(Bytes::from),
                    slots: HashMap::new(),
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
        .fetch_all(&*self.pool)
        .await?;

        for slot_row in slot_rows {
            let addr = &slot_row.account_address.try_into().unwrap_or_default(); //XXX add alert
            if let Some(account_info) = accounts.get_mut(addr) {
                account_info.slots.insert(
                    slot_row.slot_index,
                    SlotInfo {
                        value: slot_row.value.unwrap_or_default().into(),
                    },
                );
            }
        }

        self.hybrid_accounts_slots = accounts;

        Ok(())
    }


    /// Updates the in-memory state with changes from transaction execution.
    pub async fn update_state_with_execution_changes(&mut self, changes: &Vec<ExecutionAccountChanges>) -> anyhow::Result<(), sqlx::Error> {
        for change in changes {
            let address = change.address.clone(); // Assuming Address is cloneable and the correct type.

            let account_info_entry = self.hybrid_accounts_slots.entry(address).or_insert_with(|| AccountInfo {
                balance: Wei::ZERO, // Initialize with default values.
                nonce: Nonce::ZERO,
                bytecode: None,
                slots: HashMap::new(),
            });

            // Apply basic account info changes
            if let Some(nonce) = change.nonce.clone().take_modified() {
                account_info_entry.nonce = nonce;
            }
            if let Some(balance) = change.balance.clone().take_modified() {
                account_info_entry.balance = balance;
            }
            if let Some(bytecode) = change.bytecode.clone().take_modified() {
                account_info_entry.bytecode = bytecode;
            }

            // Apply slot changes
            for (slot_index, slot_change) in change.slots.clone() {
                if let Some(slot) = slot_change.take_modified() {
                    account_info_entry.slots.insert(slot_index, SlotInfo { value: slot.value });
                }
            }
        }

        Ok(())
    }

    pub async fn get_slot_at_point(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let slot = match point_in_time {
            StoragePointInTime::Present => self.hybrid_accounts_slots.get(address).map(|account_info| {
                let value = account_info.slots.get(slot_index).map(|slot_info| slot_info.value.clone()).unwrap_or_default();
                Slot {
                    index: slot_index.clone(),
                    value,
                }
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
            .fetch_optional(&*self.pool)
            .await
            .context("failed to select slot")?,
        };
        Ok(slot)
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
            },
            StoragePointInTime::Past(_number) => Account::default(),
        }
    }
}
