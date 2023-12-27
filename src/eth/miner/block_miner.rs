use std::sync::Arc;

use ethereum_types::BloomInput;
use keccak_hasher::KeccakHasher;
use nonempty::NonEmpty;

use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;
use crate::ext::not;

pub struct BlockMiner {
    storage: Arc<dyn EthStorage>,
}

impl BlockMiner {
    pub fn new(storage: Arc<dyn EthStorage>) -> Self {
        Self { storage }
    }

    /// Returns the genesis block.
    pub fn genesis() -> Block {
        Block::new_with_capacity(BlockNumber::ZERO, 1702568764, 0)
    }

    /// Mine one block with a single transaction.
    pub fn mine_with_one_transaction(&mut self, signer: Address, input: TransactionInput, execution: TransactionExecution) -> Result<Block, EthError> {
        let transactions = NonEmpty::new((signer, input, execution));
        self.mine_with_many_transactions(transactions)
    }

    /// Mine one block from one or more transactions.
    ///
    /// TODO: maybe break this in multiple functions after the logic is complete.
    pub fn mine_with_many_transactions(&mut self, transactions: NonEmpty<(Address, TransactionInput, TransactionExecution)>) -> Result<Block, EthError> {
        // init block
        let number = self.storage.increment_block_number()?;
        let block_timpestamp = transactions
            .minimum_by(|(_, _, e1), (_, _, e2)| e1.block_timestamp_in_secs.cmp(&e2.block_timestamp_in_secs))
            .2
            .block_timestamp_in_secs;
        let mut block = Block::new_with_capacity(number, block_timpestamp, transactions.len());

        // mine transactions and logs
        let mut log_index = 0;
        for (transaction_index, (signer, input, execution)) in transactions.into_iter().enumerate() {
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
                    block_number: block.header.number.clone(),
                    block_hash: block.header.hash.clone(),
                };
                mined_logs.push(mined_log);

                // increment log index
                log_index += 1;
            }

            // mine transaction
            let mined_transaction = TransactionMined {
                signer,
                input,
                execution,
                transaction_index,
                block_number: block.header.number.clone(),
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
