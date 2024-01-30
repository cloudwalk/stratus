//! Block Module
//!
//! The Block module forms the backbone of Ethereum's blockchain structure.
//! Each block in the blockchain contains a header, which holds vital
//! information about the block, and a set of transactions that the block
//! contains. This module facilitates the creation, manipulation, and
//! serialization of blocks, enabling interactions with the blockchain data
//! structure, such as querying block information or broadcasting newly mined
//! blocks.

use ethereum_types::H256;
use ethers_core::types::Block as EthersBlock;
use ethers_core::types::Transaction as EthersTransaction;
use itertools::Itertools;
use serde_json::Value as JsonValue;

use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionMined;

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<TransactionMined>,
}

impl Block {
    /// Creates a new block with the given number and transactions capacity.
    pub fn new_with_capacity(number: BlockNumber, timestamp_in_secs: u64, capacity: usize) -> Self {
        Self {
            header: BlockHeader::new(number, timestamp_in_secs),
            transactions: Vec::with_capacity(capacity),
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
