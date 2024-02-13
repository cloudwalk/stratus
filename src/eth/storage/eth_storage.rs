use anyhow::anyhow;
use async_trait::async_trait;

use super::EthStorageError;
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
use crate::eth::storage::MetrifiedStorage;

/// EVM storage operations.
#[async_trait]
pub trait EthStorage: Send + Sync {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber>;

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>>;

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>>;

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>>;

    /// Retrieves logs from the storage.
    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>>;

    /// Persist atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError>;

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()>;

    /// Resets all state to a specific block number.
    async fn reset(&self, number: BlockNumber) -> anyhow::Result<()>;

    /// Enables genesis block.
    ///
    /// TODO: maybe can use save_block from a default method.
    async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()>;

    /// Enables test accounts.
    ///
    /// TODO: maybe can use save_accounts from a default method.
    async fn enable_test_accounts(&self, test_accounts: Vec<Account>) -> anyhow::Result<()>;

    // -------------------------------------------------------------------------
    // Default operations
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage. Returns default value when not found.
    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        match self.maybe_read_account(address, point_in_time).await? {
            Some(account) => Ok(account),
            None => {
                tracing::error!("Account not found: {}", address);
                Ok(Account { //XXX maybe we should just return an error
                    address: address.clone(),
                    ..Account::default()
                })
            },
        }
    }

    /// Retrieves an slot from the storage. Returns default value when not found.
    async fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        match self.maybe_read_slot(address, slot_index, point_in_time).await? {
            Some(slot) => Ok(slot),
            None => Ok(Slot {
                index: slot_index.clone(),
                ..Default::default()
            }),
        }
    }

    /// Wraps the current storage with a proxy that collects execution metrics.
    fn metrified(self) -> MetrifiedStorage<Self>
    where
        Self: Sized,
    {
        MetrifiedStorage::new(self)
    }

    /// Translates a block selection to a specific storage point-in-time indicator.
    async fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.read_current_block_number().await?;
                if number <= &current_block {
                    Ok(StoragePointInTime::Past(*number))
                } else {
                    Ok(StoragePointInTime::Past(current_block))
                }
            }
            BlockSelection::Earliest | BlockSelection::Hash(_) => match self.read_block(block_selection).await? {
                Some(block) => Ok(StoragePointInTime::Past(block.header.number)),
                None => Err(anyhow!(
                    "Failed to select block because it is greater than current block number or block hash is invalid."
                )),
            },
        }
    }
}

/// Retrieves test accounts.
pub fn test_accounts() -> Vec<Account> {
    use hex_literal::hex;

    use crate::eth::primitives::Wei;

    [
        hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        hex!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
        hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
        hex!("15d34aaf54267db7d7c367839aaf71a00a2c6a65"),
        hex!("9965507d1a55bcc2695c58ba16fb37d819b0a4dc"),
        hex!("976ea74026e726554db657fa54763abd0c3a0aa9"),
    ]
    .into_iter()
    .map(|address| Account {
        address: address.into(),
        balance: Wei::TEST_BALANCE,
        ..Account::default()
    })
    .collect()
}
