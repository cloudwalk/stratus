use sqlx::{Pool, Postgres};
use std::{collections::HashMap, sync::Arc};

use crate::eth::primitives::{Address, BlockNumber, Bytes, Nonce, SlotIndex, SlotValue, Wei};

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

impl HybridHistory {
    pub async fn new(pool: Arc<Pool<Postgres>>) -> Result<Self, sqlx::Error> {
        // Initialize the structure
        let mut history = HybridHistory {
            hybrid_accounts_slots: HashMap::new(),
            pool,
        };

        // Pre-load the latest data
        history.load_latest_data().await?;

        Ok(history)
    }

    async fn load_latest_data(&mut self) -> Result<(), sqlx::Error> {

        Ok(())
    }
}
