mod account;
mod address;
mod block;
mod block_header;
mod block_number;
mod bytes;
mod chain_id;
mod difficulty;
mod execution;
mod execution_result;
mod gas;
mod hash;
mod index;
mod log;
mod log_mined;
mod logs_bloom;
mod miner_nonce;
mod nonce;
mod size;
mod slot;
mod transaction_input;
mod transaction_mined;
mod unix_time;
mod wei;

pub use account::AccountRocksdb;
pub use address::AddressRocksdb;
pub use block::BlockRocksdb;
pub use block_number::BlockNumberRocksdb;
pub use hash::HashRocksdb;
pub use index::IndexRocksdb;
pub use slot::SlotIndexRocksdb;
pub use slot::SlotValueRocksdb;

#[cfg(test)]
mod tests {
    use block_header::BlockHeaderRocksdb;
    use bytes::BytesRocksdb;
    use chain_id::ChainIdRocksdb;
    use difficulty::DifficultyRocksdb;
    use execution::ExecutionRocksdb;
    use execution_result::ExecutionResultRocksdb;
    use gas::GasRocksdb;
    use log_mined::LogMinedRockdb;
    use logs_bloom::LogsBloomRocksdb;
    use miner_nonce::MinerNonceRocksdb;
    use nonce::NonceRocksdb;
    use size::SizeRocksdb;
    use transaction_input::TransactionInputRocksdb;
    use transaction_mined::TransactionMinedRocksdb;
    use unix_time::UnixTimeRocksdb;
    use wei::WeiRocksdb;

    use super::log::LogRocksdb;
    use super::*;
    use crate::gen_test_bincode;

    gen_test_bincode!(AccountRocksdb);
    gen_test_bincode!(AddressRocksdb);
    gen_test_bincode!(BlockHeaderRocksdb);
    gen_test_bincode!(BlockNumberRocksdb);
    gen_test_bincode!(BlockRocksdb);
    gen_test_bincode!(BytesRocksdb);
    gen_test_bincode!(ChainIdRocksdb);
    gen_test_bincode!(DifficultyRocksdb);
    gen_test_bincode!(ExecutionResultRocksdb);
    gen_test_bincode!(ExecutionRocksdb);
    gen_test_bincode!(GasRocksdb);
    gen_test_bincode!(HashRocksdb);
    gen_test_bincode!(IndexRocksdb);
    gen_test_bincode!(LogMinedRockdb);
    gen_test_bincode!(LogRocksdb);
    gen_test_bincode!(LogsBloomRocksdb);
    gen_test_bincode!(MinerNonceRocksdb);
    gen_test_bincode!(NonceRocksdb);
    gen_test_bincode!(SizeRocksdb);
    gen_test_bincode!(SlotIndexRocksdb);
    gen_test_bincode!(SlotValueRocksdb);
    gen_test_bincode!(TransactionInputRocksdb);
    gen_test_bincode!(TransactionMinedRocksdb);
    gen_test_bincode!(UnixTimeRocksdb);
    gen_test_bincode!(WeiRocksdb);
}
