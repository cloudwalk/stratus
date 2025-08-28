use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::bail;

use super::rocks_state::RocksStorageState;
use crate::GlobalState;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionChanges;
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
use crate::ext::SleepReason;
use crate::ext::spawn;
use crate::ext::traced_sleep;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: Arc<RocksStorageState>,
    block_number: AtomicU32,
}

fn create_file_descriptor_error_message(
    current_limit: u64,
    max_limit: u64,
    min_required: u64,
    error_context: &str,
    additional_details: Option<&str>,
) -> String {
    let details = additional_details.unwrap_or("");
    format!(
        "File descriptor limit (current: {current_limit}, max: {max_limit}) is below the recommended minimum ({min_required}) for RocksDB. \
        {error_context}{details} \
        This may cause database corruption or unexpected behavior. \
        Increase the limit with: ulimit -n {min_required} or configure it in /etc/security/limits.conf"
    )
}

fn get_current_file_descriptor_limit(min_required: u64) -> anyhow::Result<libc::rlimit> {
    let mut rlimit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    let result = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlimit as *mut libc::rlimit) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        let message = create_file_descriptor_error_message(
            0, // We don't know the current limit since getting it failed
            0, // We don't know the max limit since getting it failed
            min_required,
            &format!("Failed to get current file descriptor limit (error: {err})."),
            None,
        );
        bail!("{}", message);
    }
    Ok(rlimit)
}

fn set_file_descriptor_limit(new_rlimit: u64, current_limit: libc::rlimit, min_required: u64) -> anyhow::Result<libc::rlimit> {
    let rlimit = libc::rlimit {
        rlim_cur: new_rlimit.min(current_limit.rlim_max),
        rlim_max: current_limit.rlim_max,
    };
    let result = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlimit as *const libc::rlimit) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        let message = create_file_descriptor_error_message(
            current_limit.rlim_cur,
            current_limit.rlim_max,
            min_required,
            &format!("Failed to automatically increase the limit (error: {err})."),
            None,
        );
        bail!("{}", message);
    }
    let current_limit = get_current_file_descriptor_limit(min_required)?;
    Ok(current_limit)
}

impl RocksPermanentStorage {
    pub fn new(
        db_path_prefix: Option<String>,
        shutdown_timeout: Duration,
        cache_size_multiplier: Option<f32>,
        enable_sync_write: bool,
        cf_size_metrics_interval: Option<Duration>,
        min_file_descriptors: u64,
    ) -> anyhow::Result<Self> {
        tracing::info!("setting up rocksdb storage");

        // Check file descriptor limit before proceeding with RocksDB initialization
        Self::check_file_descriptor_limit(min_file_descriptors)?;

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

        let state = Arc::new(RocksStorageState::new(path, shutdown_timeout, cache_size_multiplier, enable_sync_write)?);

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
    fn check_file_descriptor_limit(min_required: u64) -> anyhow::Result<()> {
        let current_limit = get_current_file_descriptor_limit(min_required)?;

        tracing::info!(
            current_limit = current_limit.rlim_cur,
            min_required = min_required,
            "checking file descriptor limit for RocksDB"
        );

        if current_limit.rlim_cur < min_required {
            tracing::warn!(
                current_limit = current_limit.rlim_cur,
                min_required = min_required,
                "File descriptor limit is below minimum, attempting to increase it"
            );

            let new_rlimit = set_file_descriptor_limit(min_required, current_limit, min_required)?;

            if new_rlimit.rlim_cur < min_required {
                let message = create_file_descriptor_error_message(
                    current_limit.rlim_cur,
                    current_limit.rlim_max,
                    min_required,
                    "Attempted to increase the limit but verification shows it's still insufficient",
                    Some(&format!(" ({}).", new_rlimit.rlim_cur)),
                );
                bail!("{}", message);
            }

            tracing::info!(
                "Successfully increased file descriptor limit to {} (required: {})",
                new_rlimit.rlim_cur,
                min_required
            );
        } else {
            tracing::info!("File descriptor limit check passed: {} >= {}", current_limit.rlim_cur, min_required);
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

    pub fn read_block_with_changes(&self, selection: BlockFilter) -> anyhow::Result<Option<(Block, ExecutionChanges)>, StorageError> {
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

    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>, StorageError> {
        self.state.read_logs(filter).map_err(|err| StorageError::RocksError { err }).inspect_err(|e| {
            tracing::error!(reason = ?e, "failed to read log in RocksPermanent");
        })
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, account_changes: ExecutionChanges) -> anyhow::Result<(), StorageError> {
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

    pub fn save_block(&self, block: Block, account_changes: ExecutionChanges) -> anyhow::Result<(), StorageError> {
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
    fn test_ulimit_check_function_exists() {
        let result = RocksPermanentStorage::check_file_descriptor_limit(1024);
        assert!(result.is_ok(), "ulimit check should succeed with reasonable low limit");
    }

    #[test]
    fn test_ulimit_check_with_higher_requirement() {
        let result = RocksPermanentStorage::check_file_descriptor_limit(65_536);
        assert!(result.is_ok(), "ulimit check should attempt to increase the limit and succeed if possible");
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
