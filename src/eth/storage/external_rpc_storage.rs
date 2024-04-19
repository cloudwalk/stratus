use async_trait::async_trait;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;

#[async_trait]
pub trait ExternalRpcStorage: Send + Sync {
    /// Read the largest block number saved inside a block range.
    async fn read_max_block_number_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>>;

    /// Read all block inside a block range.
    async fn read_blocks_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalBlock>>;

    /// Read all receipts inside a block range.
    async fn read_receipts_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalReceipt>>;

    /// Read all initial accounts saved.
    async fn read_initial_accounts(&self) -> anyhow::Result<Vec<Account>>;

    /// Saves an initial account with its starting balance.
    async fn save_initial_account(&self, address: Address, balance: Wei) -> anyhow::Result<()>;

    /// Save an external block and its receipts to the storage.
    async fn save_block_and_receipts(&self, number: BlockNumber, block: JsonValue, receipts: Vec<(Hash, ExternalReceipt)>) -> anyhow::Result<()>;
}
