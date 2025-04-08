use display_json::DebugAsJson;
use indexmap::IndexMap;
use keccak_hasher::KeccakHasher;

use super::Block;
use super::Index;
use super::LogMined;
use super::Size;
use super::TransactionMined;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::TransactionExecution;

/// Block that is being mined and receiving updates.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlock {
    pub header: PendingBlockHeader,
    // TODO: review why we use an indexmap here but not everywhere else
    pub transactions: IndexMap<Hash, TransactionExecution>,
    pub external_block: Option<ExternalBlock>,
}

impl PendingBlock {
    /// Creates a new [`PendingBlock`] with the specified number.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self {
            header: PendingBlockHeader::new_at_now(number),
            transactions: IndexMap::new(),
            external_block: None,
        }
    }

    /// Adds a transaction execution to the block.
    pub fn push_transaction(&mut self, tx: TransactionExecution) {
        self.transactions.insert(tx.input.hash, tx);
    }
}

impl From<PendingBlock> for Block {
    fn from(value: PendingBlock) -> Self {
        let mut block = Block::new(value.header.number, *value.header.timestamp);
        let txs: Vec<TransactionExecution> = value.transactions.into_values().collect();
        block.transactions.reserve(txs.len());
        block.header.size = Size::from(txs.len() as u64);

        // mine transactions and logs
        let mut log_index = Index::ZERO;
        for (tx_idx, tx) in txs.into_iter().enumerate() {
            let transaction_index = Index::new(tx_idx as u64);
            // mine logs
            let mut mined_logs = Vec::with_capacity(tx.result.execution.logs.len());
            for mined_log in tx.result.execution.logs.clone() {
                // calculate bloom
                block.header.bloom.accrue_log(&mined_log);

                // mine log
                let mined_log = LogMined::mine_log(mined_log, block.number(), block.hash(), &tx, log_index, transaction_index);
                mined_logs.push(mined_log);

                // increment log index
                log_index = log_index + Index::ONE;
            }

            // mine transaction
            let mined_transaction = TransactionMined {
                input: tx.input,
                execution: tx.result.execution,
                transaction_index,
                block_number: block.header.number,
                block_hash: block.header.hash,
                logs: mined_logs,
            };

            // add transaction to block
            block.transactions.push(mined_transaction);
        }

        // calculate transactions hash
        if !block.transactions.is_empty() {
            let transactions_hashes: Vec<Hash> = block.transactions.iter().map(|x| x.input.hash).collect();
            block.header.transactions_root = triehash::ordered_trie_root::<KeccakHasher, _>(transactions_hashes).into();
        }

        // calculate final block hash

        // replicate calculated block hash from header to transactions and logs
        for transaction in block.transactions.iter_mut() {
            transaction.block_hash = block.header.hash;
            for log in transaction.logs.iter_mut() {
                log.block_hash = block.header.hash;
            }
        }

        block
    }
}
