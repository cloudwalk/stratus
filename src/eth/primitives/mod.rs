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
pub use log_mined::LogMined;
pub use log_topic::LogTopic;
pub use nonce::Nonce;
pub use slot::Slot;
pub use slot::SlotIndex;
pub use slot::SlotValue;
pub use storage_point_in_time::StoragerPointInTime;
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
    use crate::test_serde;

    type TransactionExecutionValueChangeBytes = TransactionExecutionValueChange<Bytes>;
    type TransactionExecutionValueChangeNonce = TransactionExecutionValueChange<Nonce>;
    type TransactionExecutionValueChangeOptionString = TransactionExecutionValueChange<Option<String>>;
    type TransactionExecutionValueChangeSlot = TransactionExecutionValueChange<Slot>;
    type TransactionExecutionValueChangeWei = TransactionExecutionValueChange<Wei>;

    test_serde!(Address);
    test_serde!(Block);
    test_serde!(BlockHeader);
    test_serde!(BlockNumber);
    test_serde!(Bytes);
    test_serde!(ChainId);
    test_serde!(Gas);
    test_serde!(Hash);
    test_serde!(Log);
    test_serde!(LogMined);
    test_serde!(LogTopic);
    test_serde!(Nonce);
    test_serde!(Slot);
    test_serde!(SlotIndex);
    test_serde!(SlotValue);
    test_serde!(TransactionExecutionAccountChanges);
    test_serde!(TransactionExecutionResult);
    test_serde!(TransactionExecutionValueChangeBytes);
    test_serde!(TransactionExecutionValueChangeNonce);
    test_serde!(TransactionExecutionValueChangeOptionString);
    test_serde!(TransactionExecutionValueChangeSlot);
    test_serde!(TransactionExecutionValueChangeWei);
    test_serde!(TransactionInput);
    test_serde!(TransactionMined);
    test_serde!(Wei);
}
