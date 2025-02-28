use std::path::Path;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;

use super::rocks_state::RocksStorageState;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;

/// Maximum number of replication logs to return in a single call to `get_updates_since`
const MAX_REPLICATION_LOGS: usize = 5;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: RocksStorageState,
    block_number: AtomicU32,
}

impl RocksPermanentStorage {
    pub fn new(
        db_path_prefix: Option<String>,
        shutdown_timeout: Duration,
        cache_size_multiplier: Option<f32>,
        enable_sync_write: bool,
    ) -> anyhow::Result<Self> {
        tracing::info!("setting up rocksdb storage");

        let path = if let Some(prefix) = db_path_prefix {
            // run some checks on the given prefix
            if prefix.is_empty() {
                bail!("given prefix for RocksDB is empty, try not providing the flag");
            }

            if Path::new(&prefix).is_dir() || Path::new(&prefix).iter().count() > 1 {
                tracing::warn!(?prefix, "given prefix for RocksDB might put it in another folder");
            }

            let path = format!("{prefix}-rocksdb");
            tracing::info!("starting rocksdb storage - at custom path: '{:?}'", path);
            path
        } else {
            tracing::info!("starting rocksdb storage - at default path: 'data/rocksdb'");
            "data/rocksdb".to_string()
        };

        let state = RocksStorageState::new(path, shutdown_timeout, cache_size_multiplier, enable_sync_write)?;
        let block_number = state.preload_block_number()?;

        Ok(Self { state, block_number })
    }
}

impl PermanentStorage for RocksPermanentStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber, StorageError> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.block_number.store(number.as_u32(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    fn read_account(&self, address: Address, point_in_time: PointInTime) -> anyhow::Result<Option<Account>, StorageError> {
        self.state
            .read_account(address, point_in_time)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read account in RocksPermanent");
            })
    }

    fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> anyhow::Result<Option<Slot>, StorageError> {
        self.state
            .read_slot(address, index, point_in_time)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read slot in RocksPermanent");
            })
    }

    fn read_block(&self, selection: BlockFilter) -> anyhow::Result<Option<Block>, StorageError> {
        let block = self.state.read_block(selection).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read block in RocksPermanent");
        });
        if let Ok(Some(block)) = &block {
            tracing::trace!(?selection, ?block, "block found");
        }
        block.map_err(|err| StorageError::RocksError { err })
    }

    fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>, StorageError> {
        self.state
            .read_transaction(hash)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read transaction in RocksPermanent");
            })
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>, StorageError> {
        self.state.read_logs(filter).map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read log in RocksPermanent");
        })
    }

    fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        #[cfg(feature = "rocks_metrics")]
        {
            self.state.export_metrics().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to export metrics in RocksPermanent");
            })?;
        }
        self.state.save_block(block).map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to save block in RocksPermanent");
        })
    }

    fn save_block_batch(&self, block_batch: Vec<Block>) -> anyhow::Result<(), StorageError> {
        self.state
            .save_block_batch(block_batch)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save block_batch in RocksPermanent");
            })
    }

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<(), StorageError> {
        self.state
            .save_accounts(accounts)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save accounts in RocksPermanent");
            })
    }

    #[cfg(feature = "dev")]
    fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.block_number.store(0u32, Ordering::SeqCst);
        self.state.reset().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to reset in RocksPermanent");
        })
    }

    fn get_latest_sequence_number(&self) -> anyhow::Result<u64, StorageError> {
        Ok(self.state.db.latest_sequence_number())
    }

    fn get_updates_since(&self, seq_number: u64) -> anyhow::Result<Vec<(u64, Vec<u8>)>, StorageError> {
        let wal_iterator = self
            .state
            .db
            .get_updates_since(seq_number)
            .map_err(|err| StorageError::RocksError { err: err.into() })?;

        let mut updates = Vec::new();
        for update in wal_iterator {
            match update {
                Ok((seq, write_batch)) => {
                    let data = write_batch.data().to_vec();
                    updates.push((seq, data));

                    // Limit the number of logs returned
                    if updates.len() >= MAX_REPLICATION_LOGS {
                        tracing::debug!("Reached maximum log limit of {MAX_REPLICATION_LOGS}, truncating results");
                        break;
                    }
                }
                Err(err) => {
                    return Err(StorageError::RocksError { err: err.into() });
                }
            }
        }

        Ok(updates)
    }

    /// TODO: add some kind of check for sequence number to avoid applying the same log twice
    fn apply_replication_log(&self, sequence: u64, log_data: Vec<u8>) -> anyhow::Result<BlockNumber, StorageError> {
        tracing::info!(sequence = %sequence, "applying replication log");
    
        let write_batch = rocksdb::WriteBatch::from_data(&log_data);
        self.state
            .write_in_batch_for_multiple_cfs(write_batch)
            .map_err(|err| StorageError::RocksError { err })?;
    
        let block_number = self.state.preload_block_number()
            .map_err(|err| StorageError::RocksError { err })?
            .load(Ordering::SeqCst);
        
        self.block_number.store(block_number, Ordering::SeqCst);
        
        tracing::info!(block_number = %block_number, "updated block number after replication");
        Ok(block_number.into())
    }

    fn create_checkpoint(&self, checkpoint_dir: &std::path::Path) -> anyhow::Result<(), StorageError> {
        use super::rocks_checkpoint::RocksCheckpoint;

        let checkpoint = RocksCheckpoint::new(Arc::clone(&self.state.db), checkpoint_dir.to_path_buf());

        checkpoint.create_checkpoint()
    }
}
