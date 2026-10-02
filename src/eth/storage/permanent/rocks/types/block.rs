use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::block_header::BlockHeaderRocksdb;
use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::transaction_mined::TransactionMinedRocksdb;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockHeader;
use crate::eth::types::BlockNumber;
use crate::eth::types::Hash;
use crate::eth::types::TransactionMined;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
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
        let header = BlockHeader {
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
        };
        let transactions = item
            .transactions
            .into_iter()
            .map(|tx| TransactionMined::from_rocks_primitives(tx, header.number.into(), header.hash.into()))
            .collect();
        Block { header, transactions }
    }
}

impl SerializeDeserializeWithContext for BlockRocksdb {}

#[cfg(test)]
mod tests {
    use super::BlockRocksdb;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::Index;
    use crate::eth::types::Log;
    use crate::eth::types::TransactionMined;
    use crate::ext::to_json_value;
    use crate::utils::test_utils::fake_first;
    use crate::utils::test_utils::fake_list;

    /// Builds a block with a small number and transactions that have logs and small log indexes,
    /// like a leader mines. Small values are required because the storage DTO narrows the block
    /// number to `u32` and log indexes are derived from the log list.
    fn sample_block() -> Block {
        let mut block = fake_first::<Block>();
        block.header.number = BlockNumber::from(1u32);
        block.transactions = fake_list::<TransactionMined>(3);
        // give every transaction exactly two logs at a small, stable first log index, like the
        // leader does, so mined data round-trips without depending on random fixture values
        for (transaction_index, transaction) in block.transactions.iter_mut().enumerate() {
            let transaction_index = (transaction_index as u64) * 2;
            let first_log_index = Index::from(transaction_index * 10);
            transaction.mined_data.index = Index::from(transaction_index);
            transaction.mined_data.first_log_index = first_log_index;
            let log = fake_first::<Log>();
            transaction.execution.output.logs = vec![log.clone(), log];
        }
        block
    }

    /// The stratus response format serializes a block through the storage DTO and JSON, exactly
    /// like the leader serializes it and the follower deserializes it in `stratus_getBlockAndReceipts`.
    #[test]
    fn block_rocksdb_json_round_trip_is_lossless() {
        let original = sample_block();

        // first hop: the same serialization the leader performs in the RPC handler
        let json = to_json_value(BlockRocksdb::from(original.clone()));
        let first: Block = serde_json::from_value::<BlockRocksdb>(json).expect("deserialize from json").into();

        // second hop must be a fixed point: nothing is canonicalized further
        let second: Block = BlockRocksdb::from(first.clone()).into();
        assert_eq!(first, second);

        // the header is copied directly, field by field
        assert_eq!(first.header, original.header);

        // mined data is rebuilt from the DTO fields, preserving the leader invariants
        for (original_transaction, rebuilt_transaction) in original.transactions.iter().zip(first.transactions.iter()) {
            assert_eq!(rebuilt_transaction.mined_data.index, original_transaction.mined_data.index);
            assert_eq!(rebuilt_transaction.mined_data.first_log_index, original_transaction.mined_data.first_log_index);
            // the block hash is rebuilt from the header the block was read with, not the stale
            // fixture value, mirroring what the leader guarantees in real blocks
            assert_eq!(rebuilt_transaction.mined_data.block_hash, original.header.hash);
        }
    }
}
