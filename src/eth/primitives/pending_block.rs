use display_json::DebugAsJson;
use indexmap::IndexMap;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::TransactionExecution;

/// Block that is being mined and receiving updates.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlock {
    pub header: PendingBlockHeader,
    // TODO: review why we use an indexmap here but not everywhere else
    pub transactions: IndexMap<Hash, TransactionExecution>,
}

impl PendingBlock {
    /// Creates a new [`PendingBlock`] with the specified number.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self {
            header: PendingBlockHeader::new_at_now(number),
            transactions: IndexMap::new(),
        }
    }

    /// Adds a transaction execution to the block.
    pub fn push_transaction(&mut self, tx: TransactionExecution) {
        self.transactions.insert(tx.input.transaction_info.hash, tx);
    }
}
