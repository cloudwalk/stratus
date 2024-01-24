use std::time::Instant;

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
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::EthStorage;
use crate::infra::metrics;

/// Proxy storage that tracks metrics.
pub struct MetrifiedStorage<T: EthStorage> {
    inner: T,
}

impl<T: EthStorage> MetrifiedStorage<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: EthStorage> EthStorage for MetrifiedStorage<T> {
    fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        self.inner.read_current_block_number()
    }

    fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        self.inner.increment_block_number()
    }

    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        let start = Instant::now();
        let result = self.inner.read_account(address, point_in_time);
        metrics::inc_storage_accounts_read(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        let start = Instant::now();
        let result = self.inner.read_slot(address, slot, point_in_time);
        metrics::inc_storage_slots_read(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let start = Instant::now();
        let result = self.inner.read_block(block_selection);
        metrics::inc_storage_blocks_read(start.elapsed(), result.is_ok());
        result
    }

    fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let start = Instant::now();
        let result = self.inner.read_mined_transaction(hash);
        metrics::inc_storage_transactions_read(start.elapsed(), result.is_ok());
        result
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let start = Instant::now();
        let result = self.inner.read_logs(filter);
        metrics::inc_storage_logs_read(start.elapsed(), result.is_ok());
        result
    }

    fn save_block(&self, block: Block) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.inner.save_block(block);
        metrics::inc_storage_blocks_written(start.elapsed(), result.is_ok());
        result
    }
}
