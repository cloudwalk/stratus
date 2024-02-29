//! Block Module
//!
//! The Block module forms the backbone of Ethereum's blockchain structure.
//! Each block in the blockchain contains a header, which holds vital
//! information about the block, and a set of transactions that the block
//! contains. This module facilitates the creation, manipulation, and
//! serialization of blocks, enabling interactions with the blockchain data
//! structure, such as querying block information or broadcasting newly mined
//! blocks.

use std::collections::HashMap;

use ethereum_types::H256;
use ethers_core::types::Block as EthersBlock;
use ethers_core::types::Transaction as EthersTransaction;
use itertools::Itertools;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSize;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<TransactionMined>,
}

impl Block {
    /// Creates a new block with the given number and transactions capacity.
    pub fn new_with_capacity(number: BlockNumber, timestamp: UnixTime, capacity: usize) -> Self {
        Self {
            header: BlockHeader::new(number, timestamp),
            transactions: Vec::with_capacity(capacity),
        }
    }

    /// Creates a new block based on an external block and its local transactions re-execution.
    ///
    /// TODO: this kind of conversion should be infallibe.
    pub fn from_external(block: ExternalBlock, executions: Vec<ExternalTransactionExecution>) -> anyhow::Result<Self> {
        let mut transactions = Vec::with_capacity(executions.len());
        for execution in executions {
            transactions.push(TransactionMined::from_external(execution)?);
        }
        Ok(Self {
            header: block.into(),
            transactions,
        })
    }

    /// Calculates block size by the number of transactions.
    ///
    /// TODO: statistics must be colleted to determine the ideal way to classify.
    pub fn size_by_transactions(&self) -> BlockSize {
        match self.transactions.len() {
            0 => BlockSize::Empty,
            1..=5 => BlockSize::Small,
            6..=10 => BlockSize::Medium,
            11..=15 => BlockSize::Large,
            _ => BlockSize::Huge,
        }
    }

    /// Calculates block size by consumed gas.
    ///
    /// TODO: statistics must be colleted to determine the ideal way to classify.
    pub fn size_by_gas(&self) -> BlockSize {
        match self.header.gas.as_u64() {
            0 => BlockSize::Empty,
            1..=499_999 => BlockSize::Small,
            500_000..=999_999 => BlockSize::Medium,
            1_000_000..=1_999_999 => BlockSize::Large,
            _ => BlockSize::Huge,
        }
    }

    /// Serializes itself to JSON-RPC block format with full transactions included.
    pub fn to_json_rpc_with_full_transactions(self) -> JsonValue {
        let json_rpc_format: EthersBlock<EthersTransaction> = self.into();
        serde_json::to_value(json_rpc_format).unwrap()
    }

    /// Serializes itself to JSON-RPC block format with only transactions hashes included.
    pub fn to_json_rpc_with_transactions_hashes(self) -> JsonValue {
        let json_rpc_format: EthersBlock<H256> = self.into();
        serde_json::to_value(json_rpc_format).unwrap()
    }

    /// Returns the block number.
    pub fn number(&self) -> &BlockNumber {
        &self.header.number
    }

    /// Returns the block hash.
    pub fn hash(&self) -> &Hash {
        &self.header.hash
    }

    /// Compact accounts changes removing intermediate values, keeping only the last modified nonce, balance, bytecode and slots.
    pub fn compact_account_changes(&self) -> Vec<ExecutionAccountChanges> {
        let mut block_compacted_changes: HashMap<Address, ExecutionAccountChanges> = HashMap::new();
        for transaction in &self.transactions {
            for transaction_changes in transaction.execution.changes.clone().into_iter() {
                let account_compacted_changes = block_compacted_changes
                    .entry(transaction_changes.address.clone())
                    .or_insert(transaction_changes.clone());
                if let Some(nonce) = transaction_changes.nonce.take_modified() {
                    account_compacted_changes.nonce.set_modified(nonce);
                }
                if let Some(balance) = transaction_changes.balance.take_modified() {
                    account_compacted_changes.balance.set_modified(balance);
                }
                if let Some(bytecode) = transaction_changes.bytecode.take_modified() {
                    account_compacted_changes.bytecode.set_modified(bytecode);
                }
                for (slot_index, slot) in transaction_changes.slots {
                    let slot_compacted_changes = account_compacted_changes.slots.entry(slot_index).or_insert(slot.clone());
                    if let Some(slot_value) = slot.take_modified() {
                        slot_compacted_changes.set_modified(slot_value);
                    }
                }
            }
        }
        block_compacted_changes.into_values().collect_vec()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Block> for EthersBlock<EthersTransaction> {
    fn from(block: Block) -> Self {
        let ethers_block = EthersBlock::<EthersTransaction>::from(block.header.clone());
        let ethers_block_transactions: Vec<EthersTransaction> = block.transactions.clone().into_iter().map_into().collect();
        Self {
            transactions: ethers_block_transactions,
            ..ethers_block
        }
    }
}

impl From<Block> for EthersBlock<H256> {
    fn from(block: Block) -> Self {
        let ethers_block = EthersBlock::<H256>::from(block.header);
        let ethers_block_transactions: Vec<H256> = block.transactions.into_iter().map(|x| x.input.hash).map_into().collect();
        Self {
            transactions: ethers_block_transactions,
            ..ethers_block
        }
    }
}
