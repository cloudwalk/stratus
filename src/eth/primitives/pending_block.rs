use display_json::DebugAsJson;
use indexmap::IndexMap;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::TransactionExecution;

/// Block that is being mined and receiving updates.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlock {
    pub number: BlockNumber,
    pub tx_executions: IndexMap<Hash, TransactionExecution>,
    pub external_block: Option<ExternalBlock>,
}

impl PendingBlock {
    /// Creates a new [`PendingBlock`] with the specified number.
    pub fn new(number: BlockNumber) -> Self {
        Self { number, ..Default::default() }
    }

    /// Adds a transaction execution to the block.
    pub fn push_transaction(&mut self, tx: TransactionExecution) {
        self.tx_executions.insert(tx.hash(), tx);
    }

    /// Splits transactions executions in local and external executions.
    pub fn split_transactions(&self) -> (Vec<LocalTransactionExecution>, Vec<ExternalTransactionExecution>) {
        let mut local_txs = Vec::with_capacity(self.tx_executions.len());
        let mut external_txs = Vec::with_capacity(self.tx_executions.len());

        for tx in self.tx_executions.values().cloned() {
            match tx {
                TransactionExecution::Local(tx) => {
                    local_txs.push(tx);
                }
                TransactionExecution::External(tx) => {
                    external_txs.push(tx);
                }
            }
        }
        (local_txs, external_txs)
    }
}
