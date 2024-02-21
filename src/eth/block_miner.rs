//! Block Miner
//!
//! Responsible for the creation of new blocks in the blockchain. The BlockMiner struct handles
//! the mining process, transforming a set of transactions into a block. It plays a crucial role in
//! the transaction execution pipeline, ensuring that transactions are validated, processed, and
//! added to the blockchain in an orderly and secure manner.

use std::sync::Arc;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;

use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::eth::storage::StratusStorage;
use crate::ext::not;

pub struct BlockMiner {
    storage: Arc<StratusStorage>,
}

impl BlockMiner {
    /// Initializes a new BlockMiner with storage access.
    /// The storage component is crucial for retrieving the current state and persisting new blocks.
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        Self { storage }
    }

    /// Constructs the genesis block, the first block in the blockchain.
    /// This block serves as the foundation of the blockchain, with a fixed state and no previous block.
    pub fn genesis() -> Block {
        Block::new_with_capacity(BlockNumber::ZERO, UnixTime::from(1702568764), 0)
    }

    /// Mine one block with no transactions.
    #[cfg(feature = "evm-mine")]
    pub async fn mine_with_no_transactions(&mut self) -> anyhow::Result<Block> {
        let number = self.storage.increment_block_number().await?;
        Ok(Block::new_with_capacity(number, UnixTime::now(), 0))
    }

    /// Mine one block with a single transaction.
    /// Internally, it wraps the single transaction into a format suitable for `mine_with_many_transactions`,
    /// enabling consistent processing for both single and multiple transaction scenarios.
    pub async fn mine_with_one_transaction(&mut self, input: TransactionInput, execution: Execution) -> anyhow::Result<Block> {
        let transactions = NonEmpty::new((input, execution));
        self.mine_with_many_transactions(transactions).await
    }

    /// Mines a new block from one or more transactions.
    /// This is the core function for block creation, processing each transaction, generating the necessary logs,
    /// and finalizing the block. It is used both directly for multiple transactions and indirectly by `mine_with_one_transaction`.
    ///
    /// TODO: Future enhancements may include breaking down this method for improved readability and maintenance.
    pub async fn mine_with_many_transactions(&mut self, transactions: NonEmpty<(TransactionInput, Execution)>) -> anyhow::Result<Block> {
        // init block
        let number = self.storage.increment_block_number().await?;
        let block_timestamp = transactions
            .minimum_by(|(_, e1), (_, e2)| e1.block_timestamp.cmp(&e2.block_timestamp))
            .1
            .block_timestamp;
        let mut block = Block::new_with_capacity(number, block_timestamp, transactions.len());

        // mine transactions and logs
        let mut log_index = Index::ZERO;
        for (tx_idx, (input, execution)) in transactions.into_iter().enumerate() {
            let transaction_index = Index::new(tx_idx as u16);
            // mine logs
            let mut mined_logs: Vec<LogMined> = Vec::with_capacity(execution.logs.len());
            for mined_log in execution.logs.clone() {
                // calculate bloom
                block.header.bloom.accrue(BloomInput::Raw(mined_log.address.as_ref()));
                for topic in &mined_log.topics {
                    block.header.bloom.accrue(BloomInput::Raw(topic.as_ref()));
                }

                // mine log
                let mined_log = LogMined {
                    log: mined_log,
                    transaction_hash: input.hash.clone(),
                    transaction_index,
                    log_index,
                    block_number: block.header.number,
                    block_hash: block.header.hash.clone(),
                };
                mined_logs.push(mined_log);

                // increment log index
                log_index = log_index + Index::ONE;
            }

            // mine transaction
            let mined_transaction = TransactionMined {
                input,
                execution,
                transaction_index,
                block_number: block.header.number,
                block_hash: block.header.hash.clone(),
                logs: mined_logs,
            };

            // add transaction to block
            block.transactions.push(mined_transaction);
        }

        // calculate transactions hash
        if not(block.transactions.is_empty()) {
            let transactions_hashes: Vec<&Hash> = block.transactions.iter().map(|x| &x.input.hash).collect();
            block.header.transactions_root = triehash::ordered_trie_root::<KeccakHasher, _>(transactions_hashes).into();
        }

        // calculate final block hash

        // replicate calculated block hash from header to transactions and logs
        for transaction in block.transactions.iter_mut() {
            transaction.block_hash = block.header.hash.clone();
            for log in transaction.logs.iter_mut() {
                log.block_hash = block.header.hash.clone();
            }
        }

        Ok(block)
    }
}
