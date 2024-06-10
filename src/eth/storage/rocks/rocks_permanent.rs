use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use futures::future::join_all;

use super::rocks_state::RocksStorageState;
use super::types::AddressRocksdb;
use super::types::SlotIndexRocksdb;
use crate::config::PermanentStorageKind;
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
use crate::eth::primitives::SlotIndexes;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;

/// used for multiple purposes, such as TPS counting and backup management
const TRANSACTION_LOOP_THRESHOLD: usize = 120_000;

static TRANSACTIONS_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: RocksStorageState,
    pub block_number: AtomicU64,
}

impl RocksPermanentStorage {
    pub async fn new(rocks_path_prefix: Option<String>) -> anyhow::Result<Self> {
        tracing::info!("creating rocksdb storage");

        let state = RocksStorageState::new(rocks_path_prefix);
        state.sync_data().await?;
        let block_number = state.preload_block_number()?;
        Ok(Self { state, block_number })
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    pub fn clear(&self) {
        self.state.clear().unwrap();
        self.block_number.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl PermanentStorage for RocksPermanentStorage {
    fn kind(&self) -> PermanentStorageKind {
        PermanentStorageKind::Rocks
    }

    async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        Ok(self.state.read_account(address, point_in_time))
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %index, ?point_in_time, "reading slot");
        Ok(self.state.read_slot(address, index, point_in_time))
    }

    async fn read_slots(&self, address: &Address, indexes: &SlotIndexes, point_in_time: &StoragePointInTime) -> anyhow::Result<HashMap<SlotIndex, SlotValue>> {
        tracing::debug!(%address, indexes_len = %indexes.len(), "reading slots");

        match point_in_time {
            StoragePointInTime::Present => {
                let keys = indexes.iter().cloned().map(|idx| ((*address).into(), idx.into()));
                Ok(self
                    .state
                    .account_slots
                    .multi_get(keys)?
                    .into_iter()
                    .map(|((_, idx), value)| (idx.into(), value.into()))
                    .collect())
            }
            StoragePointInTime::Past(number) => {
                let keys = indexes.iter().cloned().map(|idx| ((*address).into(), idx.into(), (*number).into()));
                Ok(self
                    .state
                    .account_slots_history
                    .multi_get(keys)?
                    .into_iter()
                    .map(|((_, idx, _), value)| (idx.into(), value.into()))
                    .collect())
            }
        }
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        Ok(self.state.read_block(selection))
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        self.state.read_transaction(hash)
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        self.state.read_logs(filter)
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            self.state.export_metrics();
        }
        let account_changes = block.compact_account_changes();

        //TODO move those loops inside the spawn and check if speed improves
        let mut txs_batch = vec![];
        let mut logs_batch = vec![];
        for transaction in block.transactions.clone() {
            txs_batch.push((transaction.input.hash.into(), transaction.block_number.into()));
            for log in transaction.logs {
                logs_batch.push(((transaction.input.hash.into(), log.log_index.into()), transaction.block_number.into()));
            }
        }

        // save block
        let mut futures = Vec::with_capacity(9);
        let block_number = block.number();
        let txs_rocks = Arc::clone(&self.state.transactions);
        let logs_rocks = Arc::clone(&self.state.logs);
        futures.push(tokio::task::spawn_blocking(move || {
            txs_rocks.insert_batch_indexed(txs_batch, block_number.as_u64());
        }));
        futures.push(tokio::task::spawn_blocking(move || {
            logs_rocks.insert_batch_indexed(logs_batch, block_number.as_u64());
        }));

        let block_hash = block.hash();

        let blocks_by_number = Arc::clone(&self.state.blocks_by_number);
        let blocks_by_hash = Arc::clone(&self.state.blocks_by_hash);
        let mut block_without_changes = block.clone();
        for transaction in &mut block_without_changes.transactions {
            // checks if it has a contract address to keep, later this will be used to gather deployed_contract_address
            transaction.execution.changes.retain(|_, change| change.bytecode.clone().is_modified());
        }
        let block_hash_clone = block_hash;
        futures.push(tokio::task::spawn_blocking(move || {
            blocks_by_number.insert(block_number.into(), block_without_changes.into());
        }));
        futures.push(tokio::task::spawn_blocking(move || {
            blocks_by_hash.insert_batch_indexed(vec![(block_hash_clone.into(), block_number.into())], block_number.as_u64());
        }));

        futures.append(
            &mut self
                .state
                .update_state_with_execution_changes(&account_changes, block_number)
                .context("failed to update state with execution changes")?,
        );

        let previous_count = TRANSACTIONS_COUNT.load(Ordering::Relaxed);
        let _ = TRANSACTIONS_COUNT.fetch_add(block.transactions.len(), Ordering::Relaxed);
        let current_count = TRANSACTIONS_COUNT.load(Ordering::Relaxed);

        // for every multiple of TRANSACTION_LOOP_THRESHOLD transactions, send a Backup signal
        if previous_count % TRANSACTION_LOOP_THRESHOLD > current_count % TRANSACTION_LOOP_THRESHOLD {
            self.state.backup_trigger.send(()).await.unwrap();
            TRANSACTIONS_COUNT.store(0, Ordering::Relaxed);
        }

        join_all(futures).await;
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        for account in accounts {
            let (key, value) = account.into();
            self.state.accounts.insert(key, value.clone());
            self.state.accounts_history.insert((key, 0.into()), value);
        }

        Ok(())
    }

    async fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        // reset block number
        let block_number_u64: u64 = block_number.into();
        let _ = self.block_number.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if block_number_u64 <= current {
                Some(block_number_u64)
            } else {
                None
            }
        });

        self.state.reset_at(block_number).await
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }

    async fn read_all_slots(&self, address: &Address) -> anyhow::Result<Vec<Slot>> {
        let address: AddressRocksdb = (*address).into();
        Ok(self
            .state
            .account_slots
            .iter_from((address, SlotIndexRocksdb::from(0)), rocksdb::Direction::Forward)
            .take_while(|((addr, _), _)| &address == addr)
            .map(|((_, idx), value)| Slot {
                index: idx.into(),
                value: value.into(),
            })
            .collect())
    }
}
