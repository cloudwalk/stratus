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

use super::Execution;
use super::LogMined;
use super::TransactionInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
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
    pub fn from_external(block: &ExternalBlock, executions: Vec<ExternalTransactionExecution>) -> anyhow::Result<Self> {
        let mut transactions = Vec::with_capacity(executions.len());
        for execution in executions {
            transactions.push(TransactionMined::from_external(execution)?);
        }
        Ok(Self {
            header: block.try_into()?,
            transactions,
        })
    }

    /// Pushes a single transaction execution to the blocks transactions
    pub fn push_execution(&mut self, input: TransactionInput, execution: Execution) {
        let transaction_index = (self.transactions.len() as u64).into();
        self.transactions.push(TransactionMined {
            logs: execution
                .logs
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, log)| LogMined {
                    log_index: (i as u64).into(),
                    log,
                    transaction_hash: input.hash,
                    transaction_index,
                    block_number: self.header.number,
                    block_hash: self.header.hash,
                })
                .collect(),
            input,
            execution,
            transaction_index,
            block_number: self.header.number,
            block_hash: self.header.hash,
        }); // TODO: update logs bloom
    }

    /// Calculates block size label by the number of transactions.
    pub fn label_size_by_transactions(&self) -> &'static str {
        match self.transactions.len() {
            0 => "0",
            1..=5 => "1-5",
            6..=10 => "6-10",
            11..=15 => "11-15",
            16..=20 => "16-20",
            _ => "20+",
        }
    }

    /// Calculates block size label by consumed gas.
    pub fn label_size_by_gas(&self) -> &'static str {
        match self.header.gas_used.as_u64() {
            0 => "0",
            1..=1_000_000 => "0-1M",
            1_000_001..=2_000_000 => "1M-2M",
            2_000_001..=3_000_000 => "2M-3M",
            3_000_001..=4_000_000 => "3M-4M",
            4_000_001..=5_000_000 => "4M-5M",
            5_000_001..=6_000_000 => "5M-6M",
            6_000_001..=7_000_000 => "6M-7M",
            7_000_001..=8_000_000 => "7M-8M",
            8_000_001..=9_000_000 => "8M-9M",
            9_000_001..=10_000_000 => "9M-10M",
            _ => "10M+",
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
            for transaction_changes in transaction.execution.changes.values().cloned() {
                let account_compacted_changes = block_compacted_changes
                    .entry(transaction_changes.address)
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
