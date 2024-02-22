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

use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
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

    /// Compacts all intermediate changes from an account, returning the first previous value as the definitive original value and the last modified value as the definitive modified value.
    pub fn generate_execution_changes(&self) -> Vec<ExecutionAccountChanges> {
        let mut temp_map: HashMap<&Address, ExecutionAccountChanges> = HashMap::new();
        for transaction in &self.transactions {
            let is_success = transaction.is_success();
            for change in &transaction.execution.changes {
                let address = &change.address;
                // update existent change
                if let Some(c) = temp_map.get_mut(address) {
                    let mut temp_change = c.clone();
                    if let Some(new_nonce) = change.nonce.clone().take_modified() {
                        temp_change.nonce.set_modified(new_nonce);
                    }
                    if let Some(new_balance) = change.balance.clone().take_modified() {
                        temp_change.balance.set_modified(new_balance);
                    }
                    if is_success {
                        if let Some(new_bytecode) = change.bytecode.clone().take_modified() {
                            temp_change.bytecode.set_modified(new_bytecode);
                        }
                        for (slot_index, slot) in &change.slots {
                            if let Some(slot_change) = temp_change.slots.get_mut(slot_index) {
                                if let Some(new_slot) = slot.clone().take_modified() {
                                    slot_change.set_modified(new_slot);
                                }
                            }
                        }
                    }
                    temp_map.insert(address, temp_change);
                } else {
                    let mut temp_change = change.clone();
                    // insert first change
                    if is_success {
                        if let Some(new_bytecode) = change.bytecode.clone().take_modified() {
                            temp_change.bytecode.set_modified(new_bytecode);
                        }
                        for (slot_index, slot) in &change.slots {
                            if let Some(slot_change) = temp_change.slots.get_mut(slot_index) {
                                if let Some(new_slot) = slot.clone().take_modified() {
                                    slot_change.set_modified(new_slot);
                                }
                            }
                        }
                    }
                    temp_map.insert(address, temp_change);
                }
            }
        }
        temp_map.into_values().collect_vec()
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
