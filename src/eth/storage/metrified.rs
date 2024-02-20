use std::time::Instant;

use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::EthStorage;
use crate::eth::storage::EthStorageError;
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

#[async_trait]
impl<T: EthStorage> EthStorage for MetrifiedStorage<T> {
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let start = Instant::now();
        let result = self.inner.check_conflicts(execution).await;
        metrics::inc_storage_check_conflicts(start.elapsed(), result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = self.inner.read_current_block_number().await;
        metrics::inc_storage_read_current_block_number(start.elapsed(), result.is_ok());
        result
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = self.inner.increment_block_number().await;
        metrics::inc_storage_increment_block_number(start.elapsed(), result.is_ok());
        result
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        let start = Instant::now();
        let result = self.inner.read_account(address, point_in_time).await;
        metrics::inc_storage_read_account(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let start = Instant::now();
        let result = self.inner.maybe_read_account(address, point_in_time).await;
        metrics::inc_storage_maybe_read_account(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    async fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        let start = Instant::now();
        let result = self.inner.read_slot(address, slot, point_in_time).await;
        metrics::inc_storage_read_slot(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let start = Instant::now();
        let result = self.inner.maybe_read_slot(address, slot_index, point_in_time).await;
        metrics::inc_storage_maybe_read_slot(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let start = Instant::now();
        let result = self.inner.read_block(block_selection).await;
        metrics::inc_storage_read_block(start.elapsed(), result.is_ok());
        result
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let start = Instant::now();
        let result = self.inner.read_mined_transaction(hash).await;
        metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());
        result
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let start = Instant::now();
        let result = self.inner.read_logs(filter).await;
        metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());
        result
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let start = Instant::now();
        let result = self.inner.save_block(block).await;
        metrics::inc_storage_save_block(start.elapsed(), result.is_ok());
        result
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.inner.save_accounts(accounts).await;
        metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());
        result
    }

    async fn save_account_changes(&self, number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.inner.save_account_changes(number, execution).await;
        metrics::inc_storage_save_account_changes(start.elapsed(), result.is_ok());
        result
    }

    async fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.inner.reset(number).await;
        metrics::inc_storage_reset(start.elapsed(), result.is_ok());
        result
    }

    /// Not relevant to track.
    async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()> {
        self.inner.enable_genesis(genesis).await
    }
}
