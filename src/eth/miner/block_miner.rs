use std::sync::Arc;

use chrono::DateTime;
use keccak_hasher::KeccakHasher;

use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::EthError;

pub struct BlockMiner {
    storage: Arc<dyn BlockNumberStorage>,
}

impl BlockMiner {
    pub fn new(storage: Arc<dyn BlockNumberStorage>) -> Self {
        Self { storage }
    }

    /// Returns the genesis block.
    pub fn genesis() -> Block {
        let mut block = Block::new(BlockNumber::ZERO);
        block.header.created_at = DateTime::from_timestamp(1702568764, 0).unwrap();
        block
    }

    /// Mine one block with a single transaction.
    pub fn mine_with_one_transaction(&mut self, signer: Address, input: TransactionInput, execution: TransactionExecution) -> Result<Block, EthError> {
        self.mine_with_many_transactions(vec![(signer, input, execution)])
    }

    /// Mine one block from one or more transactions.
    pub fn mine_with_many_transactions(&mut self, transactions: Vec<(Address, TransactionInput, TransactionExecution)>) -> Result<Block, EthError> {
        // prepare base block
        let number = self.storage.increment_block_number()?;
        let mut block = Block::new_with_capacity(number, transactions.len());
        let header = &mut block.header;

        // add transactions to block
        for (index, (signer, input, execution)) in transactions.into_iter().enumerate() {
            let trx = TransactionMined {
                signer,
                input,
                execution,
                index_in_block: index,
                block_number: header.number.clone(),
                block_hash: header.hash.clone(),
            };
            block.transactions.push(trx);
        }

        // calculate transactions hash
        if !block.transactions.is_empty() {
            let transactions_hashes: Vec<&Hash> = block.transactions.iter().map(|x| &x.input.hash).collect();
            header.transactions_root = triehash::ordered_trie_root::<KeccakHasher, _>(transactions_hashes).into();
        }

        // calculate final block hash

        // replicate hash from block header to transactions
        for transaction in block.transactions.iter_mut() {
            transaction.block_hash = block.header.hash.clone();
        }

        Ok(block)
    }
}
