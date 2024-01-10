//! Ethereum and EVM entities.

mod account;
mod address;
mod alias;
mod block;
mod block_header;
mod block_number;
mod block_selection;
mod bytes;
mod call_input;
mod chain_id;
mod gas;
mod hash;
mod historical_value;
mod log;
mod log_filter;
mod log_filter_input;
mod log_mined;
mod log_topic;
mod nonce;
mod slot;
mod storage_point_in_time;
mod transaction_execution;
mod transaction_input;
mod transaction_mined;
mod wei;

pub use account::Account;
pub use address::Address;
pub use alias::Signature32Bytes;
pub use alias::Signature4Bytes;
pub use alias::SoliditySignature;
pub use block::Block;
pub use block_header::BlockHeader;
pub use block_number::BlockNumber;
pub use block_selection::BlockSelection;
pub use bytes::Bytes;
pub use call_input::CallInput;
pub use chain_id::ChainId;
pub use gas::Gas;
pub use hash::Hash;
pub use historical_value::HistoricalValue;
pub use historical_value::HistoricalValues;
pub use log::Log;
pub use log_filter::LogFilter;
pub use log_filter_input::LogFilterInput;
pub use log_mined::LogMined;
pub use log_topic::LogTopic;
pub use nonce::Nonce;
pub use slot::Slot;
pub use slot::SlotIndex;
pub use slot::SlotValue;
pub use storage_point_in_time::StoragePointInTime;
pub use transaction_execution::ExecutionChanges;
pub use transaction_execution::TransactionExecution;
pub use transaction_execution::TransactionExecutionAccountChanges;
pub use transaction_execution::TransactionExecutionResult;
pub use transaction_execution::TransactionExecutionValueChange;
pub use transaction_input::TransactionInput;
pub use transaction_mined::TransactionMined;
pub use wei::Wei;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen_test_serde;

    type TransactionExecutionValueChangeBytes = TransactionExecutionValueChange<Bytes>;
    type TransactionExecutionValueChangeNonce = TransactionExecutionValueChange<Nonce>;
    type TransactionExecutionValueChangeOptionString = TransactionExecutionValueChange<Option<String>>;
    type TransactionExecutionValueChangeSlot = TransactionExecutionValueChange<Slot>;
    type TransactionExecutionValueChangeWei = TransactionExecutionValueChange<Wei>;

    gen_test_serde!(Address);
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
    gen_test_serde!(TransactionExecutionAccountChanges);
    gen_test_serde!(TransactionExecutionResult);
    gen_test_serde!(TransactionExecutionValueChangeBytes);
    gen_test_serde!(TransactionExecutionValueChangeNonce);
    gen_test_serde!(TransactionExecutionValueChangeOptionString);
    gen_test_serde!(TransactionExecutionValueChangeSlot);
    gen_test_serde!(TransactionExecutionValueChangeWei);
    gen_test_serde!(TransactionInput);
    gen_test_serde!(TransactionMined);
    gen_test_serde!(Wei);
}
