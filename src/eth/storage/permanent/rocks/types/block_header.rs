use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::block_number::BlockNumberRocksdb;
use super::bytes::BytesRocksdb;
use super::difficulty::DifficultyRocksdb;
use super::gas::GasRocksdb;
use super::hash::HashRocksdb;
use super::logs_bloom::LogsBloomRocksdb;
use super::miner_nonce::MinerNonceRocksdb;
use super::size::SizeRocksdb;
use super::unix_time::UnixTimeRocksdb;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderRocksdb {
    pub number: BlockNumberRocksdb,
    pub hash: HashRocksdb,
    pub transactions_root: HashRocksdb,
    pub gas_used: GasRocksdb,
    pub gas_limit: GasRocksdb,
    pub bloom: LogsBloomRocksdb,
    pub timestamp: UnixTimeRocksdb,
    pub parent_hash: HashRocksdb,
    pub author: AddressRocksdb,
    pub extra_data: BytesRocksdb,
    pub miner: AddressRocksdb,
    pub difficulty: DifficultyRocksdb,
    pub receipts_root: HashRocksdb,
    pub uncle_hash: HashRocksdb,
    pub size: SizeRocksdb,
    pub state_root: HashRocksdb,
    pub total_difficulty: DifficultyRocksdb,
    pub nonce: MinerNonceRocksdb,
}
