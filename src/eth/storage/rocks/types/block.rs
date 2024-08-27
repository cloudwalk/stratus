use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::block_header::BlockHeaderRocksdb;
use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::transaction_mined::TransactionMinedRocksdb;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockHeader;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionMined;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct BlockRocksdb {
    pub header: BlockHeaderRocksdb,
    pub transactions: Vec<TransactionMinedRocksdb>,
}

impl From<Block> for BlockRocksdb {
    fn from(item: Block) -> Self {
        BlockRocksdb {
            header: BlockHeaderRocksdb {
                number: BlockNumberRocksdb::from(item.header.number),
                hash: HashRocksdb::from(item.header.hash),
                transactions_root: HashRocksdb::from(item.header.transactions_root),
                gas_used: item.header.gas_used.into(),
                gas_limit: item.header.gas_limit.into(),
                bloom: item.header.bloom.into(),
                timestamp: item.header.timestamp.into(),
                parent_hash: HashRocksdb::from(item.header.parent_hash),
                author: AddressRocksdb::from(item.header.author),
                extra_data: item.header.extra_data.into(),
                miner: AddressRocksdb::from(item.header.miner),
                difficulty: item.header.difficulty.into(),
                receipts_root: HashRocksdb::from(item.header.receipts_root),
                uncle_hash: HashRocksdb::from(item.header.uncle_hash),
                size: item.header.size.into(),
                state_root: HashRocksdb::from(item.header.state_root),
                total_difficulty: item.header.total_difficulty.into(),
                nonce: item.header.nonce.into(),
            },
            transactions: item.transactions.into_iter().map(TransactionMinedRocksdb::from).collect(),
        }
    }
}

impl From<BlockRocksdb> for Block {
    fn from(item: BlockRocksdb) -> Self {
        Block {
            header: BlockHeader {
                number: BlockNumber::from(item.header.number),
                hash: Hash::from(item.header.hash),
                transactions_root: Hash::from(item.header.transactions_root),
                gas_used: item.header.gas_used.into(),
                gas_limit: item.header.gas_limit.into(),
                bloom: item.header.bloom.into(),
                timestamp: item.header.timestamp.into(),
                parent_hash: Hash::from(item.header.parent_hash),
                author: Address::from(item.header.author),
                extra_data: item.header.extra_data.into(),
                miner: Address::from(item.header.miner),
                difficulty: item.header.difficulty.into(),
                receipts_root: Hash::from(item.header.receipts_root),
                uncle_hash: Hash::from(item.header.uncle_hash),
                size: item.header.size.into(),
                state_root: Hash::from(item.header.state_root),
                total_difficulty: item.header.total_difficulty.into(),
                nonce: item.header.nonce.into(),
            },
            transactions: item.transactions.into_iter().map(TransactionMined::from).collect(),
        }
    }
}
