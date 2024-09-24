use display_json::DebugAsJson;
use indexmap::IndexMap;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::TransactionExecution;

/// Block that is being mined and receiving updates.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlock {
    pub header: PendingBlockHeader,
    pub transactions: IndexMap<Hash, TransactionExecution>,
    pub external_block: Option<ExternalBlock>,
}

impl PendingBlock {
    /// Creates a new [`PendingBlock`] with the specified number.
    pub fn new_with_number(number: BlockNumber) -> Self {
        Self {
            header: PendingBlockHeader::new_with_number(number),
            transactions: IndexMap::new(),
            external_block: None,
        }
    }

    /// Creates a new [`PendingBlock`] with the specified header.
    pub fn new_with_header(pending_header: PendingBlockHeader) -> Self {
        Self {
            header: pending_header,
            transactions: IndexMap::new(),
            external_block: None,
        }
    }

    /// Adds a transaction execution to the block.
    pub fn push_transaction(&mut self, tx: TransactionExecution) {
        self.transactions.insert(tx.hash(), tx);
    }
}
