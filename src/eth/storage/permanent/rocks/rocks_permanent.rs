use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::bail;

use super::rocks_cf_cache_config::RocksCfCacheConfig;
use super::rocks_state::RocksStorageState;
use super::types::BlockRocksdb;
use crate::GlobalState;
use crate::eth::executor::State;
use crate::eth::executor::types::state::Final;
use crate::eth::rpc::BlockFilter;
use crate::eth::rpc::LogFilter;
use crate::eth::storage::MinedPointInTime;
use crate::eth::storage::StorageError;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::Hash;
use crate::eth::types::LogMessage;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::TransactionMined;
#[cfg(feature = "dev")]
use crate::eth::types::Wei;
use crate::ext::SleepReason;
use crate::ext::spawn;
use crate::ext::traced_sleep;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: Arc<RocksStorageState>,
    block_number: AtomicU32,
}

fn get_current_file_descriptor_limit() -> Result<libc::rlimit, StorageError> {
    let mut rlimit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    let result = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlimit as *mut libc::rlimit) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        let msg = format!("Failed to get current file descriptor limit (error: {err}).");
        return Err(StorageError::Unexpected { msg });
    }
    Ok(rlimit)
}

impl RocksPermanentStorage {
    pub fn new(
        db_path_prefix: Option<String>,
        shutdown_timeout: Duration,
        cf_cache_config: RocksCfCacheConfig,
        enable_sync_write: bool,
        cf_size_metrics_interval: Option<Duration>,
        file_descriptors_limit: u64,
    ) -> anyhow::Result<Self> {
        tracing::info!("setting up rocksdb storage");

        // Check file descriptor limit before proceeding with RocksDB initialization
        Self::check_file_descriptor_limit(file_descriptors_limit).map_err(|err| anyhow::anyhow!("{err}"))?;

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

        let state = Arc::new(RocksStorageState::new(path, shutdown_timeout, cf_cache_config, enable_sync_write)?);

        let block_number = state.preload_block_number()?;

        // spawn background task for collecting column family size metrics
        #[cfg(feature = "metrics")]
        if let Some(interval) = cf_size_metrics_interval {
            tracing::info!("starting column family size metrics collector with interval {:?}", interval);
            spawn(
                "rocks::cf_size_metrics_collector",
                Self::start_cf_size_metrics_collector(Arc::clone(&state), interval),
            );
        };

        Ok(Self { state, block_number })
    }

    /// Checks the current file descriptor limit and validates it meets the minimum requirement.
    ///
    /// This prevents RocksDB from misbehaving or corrupting data due to insufficient file descriptors.
    fn check_file_descriptor_limit(limit_required: u64) -> Result<(), StorageError> {
        let current_limit = get_current_file_descriptor_limit()?;

        tracing::info!(
            current_limit = current_limit.rlim_cur,
            min_required = limit_required,
            "checking file descriptor limit for RocksDB"
        );

        if current_limit.rlim_cur < limit_required {
            tracing::warn!(
                current_limit = current_limit.rlim_cur,
                min_required = limit_required,
                "File descriptor limit is below minimum."
            );

            let msg = format!(
                "File descriptor limit (current: {}, max: {}) is below the recommended minimum ({limit_required}) for RocksDB.",
                current_limit.rlim_cur, current_limit.rlim_max
            );

            return Err(StorageError::Unexpected { msg });
        } else {
            tracing::info!("File descriptor limit check passed: {} >= {}", current_limit.rlim_cur, limit_required);
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    pub fn read_mined_block_number(&self) -> BlockNumber {
        self.block_number.load(Ordering::SeqCst).into()
    }

    pub fn set_mined_block_number(&self, number: BlockNumber) {
        self.block_number.store(number.as_u32(), Ordering::SeqCst);
    }

    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        let genesis = self.read_block(BlockFilter::Number(BlockNumber::ZERO))?;
        Ok(genesis.is_some())
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    pub fn read_account(&self, address: Address, point: &MinedPointInTime<'_>) -> anyhow::Result<Option<Account>, StorageError> {
        self.state
            .read_account(address, point)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read account in RocksPermanent");
            })
    }

    pub fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>, StorageError> {
        self.state.read_accounts(addresses).map_err(|err| StorageError::RocksError { err })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, point: &MinedPointInTime<'_>) -> anyhow::Result<Option<Slot>, StorageError> {
        self.state
            .read_slot(address, index, point)
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

    pub fn read_block_with_changes(&self, selection: BlockFilter) -> anyhow::Result<Option<(BlockRocksdb, BlockChangesRocksdb)>, StorageError> {
        let result = self.state.read_block_with_changes(selection).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read block with changes in RocksPermanent");
        });
        if let Ok(Some(block)) = &result {
            tracing::trace!(?selection, ?block, "block found");
        }
        result.map_err(|err| StorageError::RocksError { err })
    }

    pub fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>, StorageError> {
        self.state
            .read_transaction(hash)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to read transaction in RocksPermanent");
            })
    }

    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMessage>, StorageError> {
        self.state.read_logs(filter).map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read log in RocksPermanent");
        })
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, account_changes: State<Final>) -> anyhow::Result<(), StorageError> {
        #[cfg(feature = "rocks_metrics")]
        {
            self.state.export_metrics().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to export metrics in RocksPermanent");
            })?;
        }

        self.state
            .save_genesis_block(block, accounts, account_changes)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to save genesis block in RocksPermanent");
            })
    }

    pub fn save_block(&self, block: Block, account_changes: State<Final>) -> anyhow::Result<(), StorageError> {
        #[cfg(feature = "rocks_metrics")]
        {
            self.state.export_metrics().map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
                tracing::error!(reason = ?e, "failed to export metrics in RocksPermanent");
            })?;
        }
        self.state
            .save_block(block, account_changes)
            .map_err(|err| StorageError::RocksError { err })
            .inspect_err(|e| {
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
                tracing::warn!("failed to export column family metrics: {:?}", e);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ulimit_check_with_no_change() {
        let result = RocksPermanentStorage::check_file_descriptor_limit(1024);
        assert!(result.is_ok(), "ulimit check should succeed with reasonable low limit");
    }

    #[test]
    fn test_ulimit_check_with_max_requirement() {
        let result = RocksPermanentStorage::check_file_descriptor_limit(u64::MAX);
        assert!(
            result.is_err(),
            "ulimit check should fail because the required limit is above the system maximum"
        );
    }
}
