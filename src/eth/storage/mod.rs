//! Ethereum / EVM storage.

use cache::CacheConfig;
pub use cache::StorageCache;
pub use error::StorageError;
pub use permanent::PermanentStorageConfig;
pub use permanent::RocksPermanentStorage;
pub use stratus_storage::MinedPointInTime;
pub use stratus_storage::StratusStorage;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorageConfig;
pub use types::FoundAt;
mod cache;
mod error;
pub mod permanent;
mod resolve_pending;
mod stratus_storage;
mod temporary;
mod types;

use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
pub use temporary::compute_pending_block_number;

pub use crate::eth::types::ExecutionKind;
use crate::eth::types::StratusError;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Configuration that can be used by any binary that interacts with Stratus storage.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct StorageConfig {
    #[clap(flatten)]
    pub temp_storage: TemporaryStorageConfig,

    #[clap(flatten)]
    pub perm_storage: PermanentStorageConfig,

    #[clap(flatten)]
    pub cache: CacheConfig,
}

impl StorageConfig {
    /// Initializes Stratus storage.
    pub fn init(&self) -> Result<Arc<StratusStorage>, StratusError> {
        let perm_storage = self.perm_storage.init()?;
        let temp_storage = self.temp_storage.init(&perm_storage)?;
        let cache = self.cache.init();

        let storage = StratusStorage::new(
            temp_storage,
            perm_storage,
            cache,
            #[cfg(feature = "dev")]
            self.perm_storage.clone(),
        )?;

        Ok(Arc::new(storage))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::eth::executor::State;
    use crate::eth::executor::TransactionExecution;
    use crate::eth::executor::TransactionExecutionInput;
    use crate::eth::executor::TransactionExecutionResult;
    use crate::eth::executor::types::state::AccountChanges;
    use crate::eth::executor::types::state::Complete;
    use crate::eth::executor::types::state::CompleteValue;
    use crate::eth::storage::cache::CacheConfig;
    use crate::eth::types::Account;
    use crate::eth::types::Address;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::Signature;
    use crate::eth::types::SlotIndex;
    use crate::eth::types::SlotValue;
    use crate::eth::types::TransactionInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::Wei;

    impl StratusStorage {
        pub fn mine_block_with_mock_execution(&self, state: State<Complete>) -> BlockNumber {
            let header = self.read_pending_block_header();
            let evm_input = TransactionExecutionInput::create(&TransactionInput::default(), header);

            let result = TransactionExecutionResult {
                result: crate::eth::executor::ExecutionResult::Success,
                ..Default::default()
            };

            let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), evm_input, result);
            self.save_execution(tx, state).expect("save execution");

            let (block, block_changes) = self.finish_pending_block();
            let block_number = block.header.number;
            self.save_block(block.into(), block_changes).expect("save block");
            block_number
        }

        pub fn new_test() -> Result<Self, StorageError> {
            let temp = InMemoryTemporaryStorage::new(0.into());

            // Create a temporary directory for RocksDB
            let rocks_dir = tempdir().expect("Failed to create temporary directory for tests");
            let rocks_path_prefix = rocks_dir.path().to_str().unwrap().to_string();

            let perm = RocksPermanentStorage::new(
                Some(rocks_path_prefix.clone()),
                std::time::Duration::from_secs(240),
                super::permanent::RocksCfCacheConfig::default(),
                true,
                None,
                1024,
            )
            .expect("Failed to create RocksPermanentStorage for tests");

            let cache = CacheConfig {
                account_history_cache_capacity: 20000,
                slot_history_cache_capacity: 100000,
            }
            .init();

            Self::new(
                temp,
                perm,
                cache,
                #[cfg(feature = "dev")]
                super::permanent::PermanentStorageConfig {
                    rocks_path_prefix: Some(rocks_path_prefix),
                    rocks_shutdown_timeout: std::time::Duration::from_secs(240),
                    rocks_cf_cache: super::permanent::RocksCfCacheConfig::default(),
                    rocks_disable_sync_write: false,
                    rocks_cf_size_metrics_interval: None,
                    genesis_file: crate::config::GenesisFileConfig::default(),
                    rocks_file_descriptors_limit: 1024,
                },
            )
            .inspect(|this| {
                if !this.has_genesis().unwrap() {
                    this.mine_block_with_mock_execution(State::default());
                }
            })
        }
    }

    /// An `eth_call` pinned to a block that is no longer the latest must read the historical
    /// state at its captured block, not the current latest state.
    #[test]
    fn read_slot_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xAA; 20]);
        let index = SlotIndex::ZERO;

        // Mine a block setting slot S = 100. The eth_call captures this block.
        let mut changes1 = State::default();
        changes1
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([100u64, 0, 0, 0])));
        let call_block = storage.mine_block_with_mock_execution(changes1);

        // A new block is mined while the call is in flight, changing the slot to 200.
        let mut changes2 = State::default();
        changes2
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([200u64, 0, 0, 0])));
        let latest = storage.mine_block_with_mock_execution(changes2);
        assert_ne!(call_block, latest);

        // The in-flight call (pinned to the first block) reads the slot.
        let (slot, _) = storage.read_slot(address, index, ExecutionKind::CallLatest(call_block)).expect("read slot");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(slot.value, SlotValue::from([100u64, 0, 0, 0]));
    }

    #[test]
    fn read_account_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xBB; 20]);

        // Mine a block setting the account balance to 100. The eth_call captures this block.
        let mut changes1 = State::default();
        changes1
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(100u64))));
        let call_block = storage.mine_block_with_mock_execution(changes1);

        // A new block is mined while the call is in flight, changing the balance to 200.
        let mut changes2 = State::default();
        changes2
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(200u64))));
        let latest = storage.mine_block_with_mock_execution(changes2);
        assert_ne!(call_block, latest);

        let (account, _) = storage.read_account(address, ExecutionKind::CallLatest(call_block)).expect("read account");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(account.balance, Wei::from(100u64));
    }
}
