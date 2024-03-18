use std::collections::HashMap;
use std::sync::Arc;


use sqlx::types::BigDecimal;
use sqlx::FromRow;
use sqlx::Pool;
use sqlx::Postgres;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;

#[derive(Debug)]
struct SlotInfo {
    block_number: BlockNumber,
    value: SlotValue,
}

#[derive(Debug)]
struct AccountInfo {
    balance: Wei,
    nonce: Nonce,
    bytecode: Option<Bytes>,
    slots: HashMap<SlotIndex, SlotInfo>,
}

#[derive(Debug)]
pub struct HybridHistory {
    hybrid_accounts_slots: HashMap<Address, AccountInfo>,
    pool: Arc<Pool<Postgres>>,
}

#[derive(FromRow)]
struct AccountRow {
    address: Vec<u8>,
    nonce: Option<BigDecimal>,
    balance: Option<BigDecimal>,
    bytecode: Option<Vec<u8>>,
    block_number: BlockNumber,
}

#[derive(FromRow)]
struct SlotRow {
    account_address: Vec<u8>,
    slot_index: SlotIndex,
    value: Option<Vec<u8>>,
    block_number: BlockNumber,
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
                bytecode,
                block_number
            FROM
                neo_accounts
            ORDER BY
                address,
                block_number DESC,
                created_at DESC
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
                value,
                block_number
            FROM
                neo_account_slots
            ORDER BY
                account_address,
                slot_index,
                block_number DESC,
                created_at DESC
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
                        block_number: slot_row.block_number,
                        value: slot_row.value.unwrap_or_default().into(),
                    },
                );
            }
        }

        self.hybrid_accounts_slots = accounts;

        Ok(())
    }
}
