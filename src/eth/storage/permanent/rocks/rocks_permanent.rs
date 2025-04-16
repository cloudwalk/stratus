use std::path::Path;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use rocksdb::WriteBatch;

use super::rocks_state::RocksStorageState;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionMined;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::SleepReason;
use crate::GlobalState;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: Arc<RocksStorageState>,
    block_number: AtomicU32,
}

impl RocksPermanentStorage {
    pub fn new(
        db_path_prefix: Option<String>,
        shutdown_timeout: Duration,
        cache_size_multiplier: Option<f32>,
        enable_sync_write: bool,
        use_rocksdb_replication: bool,
        cf_size_metrics_interval: Option<Duration>,
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

        let state = Arc::new(RocksStorageState::new(
            path,
            shutdown_timeout,
            cache_size_multiplier,
            enable_sync_write,
            use_rocksdb_replication,
        )?);

        let block_number = state.preload_block_number()?;

        // spawn background task for collecting column family size metrics
        #[cfg(feature = "metrics")]
        if let Some(interval) = cf_size_metrics_interval {
            tracing::info!("starting column family size metrics collector with interval {:?}", interval);
            spawn_named(
                "rocks::cf_size_metrics_collector",
                Self::start_cf_size_metrics_collector(Arc::clone(&state), interval),
            );
        };

        Ok(Self { state, block_number })
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    pub fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber, StorageError> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    pub fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.block_number.store(number.as_u32(), Ordering::SeqCst);
        Ok(())
    }

    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        let genesis = self.read_block(BlockFilter::Number(BlockNumber::ZERO))?;
        Ok(genesis.is_some())
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    pub fn read_account(&self, address: Address, point_in_time: PointInTime) -> anyhow::Result<Option<Account>, StorageError> {
        self.state
            .read_account(address, point_in_time)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read account in RocksPermanent");
            })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> anyhow::Result<Option<Slot>, StorageError> {
        self.state
            .read_slot(address, index, point_in_time)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read slot in RocksPermanent");
            })
    }

    pub fn read_block(&self, selection: BlockFilter) -> anyhow::Result<Option<Block>, StorageError> {
        let block = self.state.read_block(selection).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read block in RocksPermanent");
        });
        if let Ok(Some(block)) = &block {
            tracing::trace!(?selection, ?block, "block found");
        }
        block.map_err(|err| StorageError::RocksError { err })
    }

    pub fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>, StorageError> {
        self.state
            .read_transaction(hash)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read transaction in RocksPermanent");
            })
    }

    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>, StorageError> {
        self.state.read_logs(filter).map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read log in RocksPermanent");
        })
    }

    pub fn read_replication_log(&self, block_number: BlockNumber) -> anyhow::Result<Option<WriteBatch>, StorageError> {
        self.state
            .read_replication_log(block_number)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read replication log in RocksPermanent");
            })
    }

    pub fn apply_replication_log(&self, block_number: BlockNumber, replication_log: WriteBatch) -> anyhow::Result<(), StorageError> {
        self.state
            .apply_replication_log(block_number, replication_log)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to apply replication log in RocksPermanent");
            })?;

        self.set_mined_block_number(block_number)?;

        Ok(())
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>) -> anyhow::Result<(), StorageError> {
        #[cfg(feature = "rocks_metrics")]
        {
            self.state.export_metrics().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to export metrics in RocksPermanent");
            })?;
        }

        self.state
            .save_genesis_block(block, accounts)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save genesis block in RocksPermanent");
            })
    }

    pub fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
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

    pub fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<(), StorageError> {
        self.state
            .save_accounts(accounts)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save accounts in RocksPermanent");
            })
    }

    pub fn rocksdb_replication_enabled(&self) -> bool {
        self.state.use_rocksdb_replication
    }

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        self.state
            .save_slot(address, slot)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save slot in RocksPermanent");
            })
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        self.state
            .save_account_nonce(address, nonce)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save account nonce in RocksPermanent");
            })
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        self.state
            .save_account_balance(address, balance)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save account balance in RocksPermanent");
            })
    }

    #[cfg(feature = "metrics")]
    /// Starts a background task that collects column family size metrics at regular intervals.
    async fn start_cf_size_metrics_collector(state: Arc<RocksStorageState>, interval: Duration) -> anyhow::Result<()> {
        const TASK_NAME: &str = "rocks::cf_size_metrics";

        loop {
            if GlobalState::is_shutdown_warn(TASK_NAME) {
                return Ok(());
            }

            if let Err(e) = state.export_column_family_size_metrics() {
                tracing::warn!("Failed to export column family metrics: {:?}", e);
            }

            traced_sleep(interval, SleepReason::Interval).await;
        }
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        self.state
            .save_account_code(address, code)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save account code in RocksPermanent");
            })
    }

    #[cfg(feature = "dev")]
    pub fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.block_number.store(0u32, Ordering::SeqCst);
        self.state.reset().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to reset in RocksPermanent");
        })
    }
}
