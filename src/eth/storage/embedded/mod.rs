use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use rocksdb::Options;
use rocksdb::DB;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

#[derive(Debug)]
pub struct EmbeddedPermanentStorage {
    blocks_db: DB,
    transactions_db: DB,
    accounts_db: DB,
    logs_db: DB,
    block_number: Arc<tokio::sync::Mutex<BlockNumber>>,
}

impl EmbeddedPermanentStorage {
    pub async fn new() -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let blocks_db = DB::open(&opts, Path::new("/tmp/blocks-db"))?;
        let transactions_db = DB::open(&opts, Path::new("/tmp/transactions-db"))?;
        let accounts_db = DB::open(&opts, Path::new("/tmp/accounts-db"))?;
        let logs_db = DB::open(&opts, Path::new("/tmp/logs-db"))?;
        let block_number = Arc::new(tokio::sync::Mutex::new(BlockNumber::from(0)));

        Ok(Self {
            blocks_db,
            transactions_db,
            accounts_db,
            logs_db,
            block_number,
        })
    }
}

#[async_trait]
impl PermanentStorage for EmbeddedPermanentStorage {
    async fn read_current_block_number(&self) -> Result<BlockNumber> {
        let num = *self.block_number.lock().await;
        Ok(num)
    }

    async fn increment_block_number(&self) -> Result<BlockNumber> {
        let mut num = self.block_number.lock().await;
        *num += 1;
        Ok(*num)
    }

    async fn set_block_number(&self, number: BlockNumber) -> Result<()> {
        let mut num = self.block_number.lock().await;
        *num = number;
        Ok(())
    }

    async fn maybe_read_account(&self, _address: &Address, _point_in_time: &StoragePointInTime) -> Result<Option<Account>> {
        // Placeholder for fetching an account. Implement based on your storage pattern.
        Ok(None)
    }

    async fn maybe_read_slot(&self, _address: &Address, _slot_index: &SlotIndex, _point_in_time: &StoragePointInTime) -> Result<Option<Slot>> {
        // Placeholder for fetching a slot. Implement based on your storage pattern.
        Ok(None)
    }

    async fn read_block(&self, _block_selection: &BlockSelection) -> Result<Option<Block>> {
        // Placeholder for fetching a block. Implement based on your storage pattern.
        Ok(None)
    }

    async fn read_mined_transaction(&self, _hash: &Hash) -> Result<Option<TransactionMined>> {
        // Placeholder for fetching a transaction. Implement based on your storage pattern.
        Ok(None)
    }

    async fn read_logs(&self, _filter: &LogFilter) -> Result<Vec<LogMined>> {
        // Placeholder for fetching logs. Implement based on your storage pattern.
        Ok(vec![])
    }

    async fn save_block(&self, _block: Block) -> Result<(), StorageError> {
        // Placeholder for saving a block. Implement based on your storage pattern.
        Ok(())
    }

    async fn save_accounts(&self, _accounts: Vec<Account>) -> Result<()> {
        // Placeholder for saving accounts. Implement based on your storage pattern.
        Ok(())
    }

    async fn reset_at(&self, _number: BlockNumber) -> Result<()> {
        // Placeholder for resetting state. Implement based on your storage pattern.
        Ok(())
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> Result<Vec<SlotSample>> {
        // Placeholder for sampling slots. Implement based on your storage pattern.
        Ok(vec![])
    }
}
