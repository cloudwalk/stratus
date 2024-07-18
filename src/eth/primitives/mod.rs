mod account;
mod address;
mod block;
mod block_filter;
mod block_header;
mod block_number;
pub mod bytes;
mod call_input;
mod chain_id;
mod code_hash;
mod difficulty;
mod ecdsa_rs;
mod ecdsa_v;
mod execution;
mod execution_account_changes;
mod execution_conflict;
mod execution_metrics;
mod execution_result;
mod execution_value_change;
mod external_block;
mod external_receipt;
mod external_receipts;
mod external_transaction;
mod gas;
mod hash;
mod index;
mod log;
mod log_filter;
mod log_filter_input;
mod log_mined;
mod log_topic;
pub mod logs_bloom;
mod miner_nonce;
mod nonce;
mod now;
mod pending_block;
mod signature;
mod size;
mod slot;
mod slot_index;
mod slot_sample;
mod slot_value;
mod storage_point_in_time;
mod transaction_execution;
mod transaction_input;
mod transaction_mined;
mod transaction_stage;
mod unix_time;
mod wei;

pub use account::test_accounts;
pub use account::Account;
pub use address::Address;
pub use block::Block;
pub use block_filter::BlockFilter;
pub use block_header::BlockHeader;
pub use block_number::BlockNumber;
pub use bytes::Bytes;
pub use call_input::CallInput;
pub use chain_id::ChainId;
pub use code_hash::CodeHash;
pub use difficulty::Difficulty;
pub use ecdsa_rs::EcdsaRs;
pub use ecdsa_v::EcdsaV;
pub use execution::EvmExecution;
pub use execution::ExecutionChanges;
pub use execution_account_changes::ExecutionAccountChanges;
pub use execution_conflict::ExecutionConflict;
pub use execution_conflict::ExecutionConflicts;
pub use execution_conflict::ExecutionConflictsBuilder;
pub use execution_metrics::ExecutionMetrics;
pub use execution_result::ExecutionResult;
pub use execution_value_change::ExecutionValueChange;
pub use external_block::ExternalBlock;
pub use external_receipt::ExternalReceipt;
pub use external_receipts::ExternalReceipts;
pub use external_transaction::ExternalTransaction;
pub use gas::Gas;
pub use hash::Hash;
pub use index::Index;
pub use log::Log;
pub use log_filter::LogFilter;
pub use log_filter_input::LogFilterInput;
pub use log_filter_input::LogFilterInputTopic;
pub use log_mined::LogMined;
pub use log_topic::LogTopic;
pub use miner_nonce::MinerNonce;
pub use nonce::Nonce;
pub use now::DateTimeNow;
pub use pending_block::PendingBlock;
pub use signature::Signature;
pub use signature::SoliditySignature;
pub use size::Size;
pub use slot::Slot;
pub use slot_index::SlotIndex;
pub use slot_sample::SlotSample;
pub use slot_value::SlotValue;
pub use storage_point_in_time::StoragePointInTime;
pub use transaction_execution::ExternalTransactionExecution;
pub use transaction_execution::LocalTransactionExecution;
pub use transaction_execution::TransactionExecution;
pub use transaction_input::TransactionInput;
pub use transaction_mined::TransactionMined;
pub use transaction_stage::TransactionStage;
pub use unix_time::UnixTime;
pub use wei::Wei;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen_test_serde;

    type TransactionExecutionValueChangeBytes = ExecutionValueChange<Bytes>;
    type TransactionExecutionValueChangeNonce = ExecutionValueChange<Nonce>;
    type TransactionExecutionValueChangeOptionString = ExecutionValueChange<Option<String>>;
    type TransactionExecutionValueChangeSlot = ExecutionValueChange<Slot>;
    type TransactionExecutionValueChangeWei = ExecutionValueChange<Wei>;

    // TODO: add more serde tests for new primitives
    gen_test_serde!(Address);
    gen_test_serde!(Account);
    gen_test_serde!(Block);
    gen_test_serde!(BlockHeader);
    gen_test_serde!(BlockNumber);
    gen_test_serde!(Bytes);
    gen_test_serde!(ChainId);
    gen_test_serde!(Gas);
    gen_test_serde!(Hash);
    gen_test_serde!(Log);
    gen_test_serde!(LogMined);
    gen_test_serde!(LogTopic);
    gen_test_serde!(Nonce);
    gen_test_serde!(Slot);
    gen_test_serde!(SlotIndex);
    gen_test_serde!(SlotValue);
    gen_test_serde!(ExecutionAccountChanges);
    gen_test_serde!(ExecutionResult);
    gen_test_serde!(TransactionExecutionValueChangeBytes);
    gen_test_serde!(TransactionExecutionValueChangeNonce);
    gen_test_serde!(TransactionExecutionValueChangeOptionString);
    gen_test_serde!(TransactionExecutionValueChangeSlot);
    gen_test_serde!(TransactionExecutionValueChangeWei);
    gen_test_serde!(TransactionInput);
    gen_test_serde!(TransactionMined);
    gen_test_serde!(Wei);

    // TODO: move these tests to block_header module
    #[test]
    fn block_hash_calculation() {
        let header = BlockHeader::new(BlockNumber::ZERO, UnixTime::from(1234567890));
        assert_eq!(header.hash.to_string(), "0x011b4d03dd8c01f1049143cf9c4c817e4b167f1d1b83e5c6f0f10d89ba1e7bce");
    }

    #[test]
    fn parent_hash() {
        let header = BlockHeader::new(BlockNumber::ONE, UnixTime::from(1234567891));
        assert_eq!(
            header.parent_hash.to_string(),
            "0x011b4d03dd8c01f1049143cf9c4c817e4b167f1d1b83e5c6f0f10d89ba1e7bce"
        );
    }

    #[test]
    fn genesis_parent_hash() {
        let header = BlockHeader::new(BlockNumber::ZERO, UnixTime::from(1234567890));
        assert_eq!(header.parent_hash, Hash::zero());
    }
}
