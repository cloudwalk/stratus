use std::collections::HashMap;

use display_json::DebugAsJson;
use ethereum_types::H256;
use itertools::Itertools;
use serde::Deserialize;

use super::LogMined;
use super::TransactionInput;
use crate::alias::EthersBlockEthersTransaction;
use crate::alias::EthersBlockH256;
use crate::alias::EthersTransaction;
use crate::alias::JsonValue;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::UnixTime;
use crate::ext::to_json_value;
use crate::log_and_err;

#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<TransactionMined>,
}

impl Block {
    /// Creates a new block with the given number and timestamp.
    pub fn new(number: BlockNumber, timestamp: UnixTime) -> Self {
        Self {
            header: BlockHeader::new(number, timestamp),
            transactions: Vec::new(),
        }
    }

    /// Constructs an empty genesis block.
    pub fn genesis() -> Block {
        Block::new(BlockNumber::ZERO, UnixTime::from(1702568764))
    }

    /// Pushes a single transaction execution to the blocks transactions.
    pub fn push_execution(&mut self, input: TransactionInput, evm_result: EvmExecutionResult) {
        let transaction_index = (self.transactions.len() as u64).into();
        self.transactions.push(TransactionMined {
            logs: evm_result
                .execution
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
            execution: evm_result.execution,
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
        let ethers_block: EthersBlockEthersTransaction = self.into();
        to_json_value(ethers_block)
    }

    /// Serializes itself to JSON-RPC block format with only transactions hashes included.
    pub fn to_json_rpc_with_transactions_hashes(self) -> JsonValue {
        let ethers_block: EthersBlockH256 = self.into();
        to_json_value(ethers_block)
    }

    /// Returns the block number.
    pub fn number(&self) -> BlockNumber {
        self.header.number
    }

    /// Returns the block hash.
    pub fn hash(&self) -> Hash {
        self.header.hash
    }

    /// Compact accounts changes removing intermediate values, keeping only the last modified nonce, balance, bytecode and slots.
    pub fn compact_account_changes(&self) -> Vec<ExecutionAccountChanges> {
        let mut block_compacted_changes: HashMap<Address, ExecutionAccountChanges> = HashMap::new();
        for transaction in &self.transactions {
            for transaction_changes in transaction.execution.changes.values() {
                let account_compacted_changes = block_compacted_changes
                    .entry(transaction_changes.address)
                    .or_insert(transaction_changes.clone());

                if let Some(&nonce) = transaction_changes.nonce.take_modified_ref() {
                    account_compacted_changes.nonce.set_modified(nonce);
                }

                if let Some(balance) = transaction_changes.balance.take_modified_ref() {
                    account_compacted_changes.balance.set_modified(*balance);
                }

                if let Some(bytecode) = transaction_changes.bytecode.take_modified_ref() {
                    account_compacted_changes.bytecode.set_modified(bytecode.clone());
                }

                for (&slot_index, slot) in &transaction_changes.slots {
                    let slot_compacted_changes = account_compacted_changes.slots.entry(slot_index).or_insert(slot.clone());
                    if let Some(&slot_value) = slot.take_modified_ref() {
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
impl From<Block> for EthersBlockEthersTransaction {
    fn from(block: Block) -> Self {
        let ethers_block = EthersBlockEthersTransaction::from(block.header.clone());
        let ethers_block_transactions: Vec<EthersTransaction> = block.transactions.clone().into_iter().map_into().collect();
        Self {
            transactions: ethers_block_transactions,
            ..ethers_block
        }
    }
}

impl From<Block> for EthersBlockH256 {
    fn from(block: Block) -> Self {
        let ethers_block = EthersBlockH256::from(block.header);
        let ethers_block_transactions: Vec<H256> = block.transactions.into_iter().map(|x| x.input.hash).map_into().collect();
        Self {
            transactions: ethers_block_transactions,
            ..ethers_block
        }
    }
}

impl TryFrom<JsonValue> for Block {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match Block::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to Block"),
        }
    }
}
