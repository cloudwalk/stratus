//! # Ethereum Primitives Module
//!
//! This module aggregates various submodules that define the primitive data structures and functionalities within the Ethereum ecosystem. It serves as a central point for organizing and exposing these primitives, ensuring a coherent and accessible interface for the application.
//!
//! ## Enumerated Modules and Their Roles
//!
//! - `account::Account`: Manages Ethereum accounts, including user wallets and contract accounts.
//! - `address::Address`: Handles Ethereum addresses, serving as unique identifiers for accounts and contracts.
//! - `alias::*`: Provides type aliases for Ethereum-specific identifiers.
//! - `block::Block`: Manages the structure of Ethereum blocks, containing transactions and a block header.
//! - `block_header::BlockHeader`: Focuses on the block header, including metadata like the parent hash, state root, etc.
//! - `block_number::BlockNumber`: Manages block numbers, tracking positions in the blockchain.
//! - `block_selection::BlockSelection`: Enables selection of specific blocks via various criteria.
//! - `bytes::Bytes`: Manages byte arrays, used for data payloads.
//! - `call_input::CallInput`: Structures input data for smart contract calls.
//! - `chain_id::ChainId`: Represents unique identifiers for different Ethereum networks.
//! - `gas::Gas`: Manages gas units for computational work and transaction fees.
//! - `hash::Hash`: Manages hash values for data integrity and blockchain consistency.
//! - `historical_value::*`: Tracks historical state changes over time.
//! - `index::Index`: Represents indexes, such as transaction positions in a block.
//! - `log::Log`: Handles logs generated during transaction execution.
//! - `log_filter::*`: Facilitates creation and querying of log filters.
//! - `log_filter_input::LogFilterInput`: Structures input for creating log filters.
//! - `log_mined::LogMined`: Represents logs included in mined blocks.
//! - `log_topic::LogTopic`: Manages log topics for categorizing and filtering logs.
//! - `logs_bloom::LogsBloom`: Manages bloom filters for efficient log searching.
//! - `nonce::Nonce`: Manages nonces for transaction ordering and replay protection.
//! - `slot::*`: Manages storage slots in contract state storage.
//! - `storage_point_in_time::StoragePointInTime`: References Ethereum storage states at different times.
//! - `transaction_execution::*`: Manages results of Ethereum transaction executions.
//! - `transaction_input::TransactionInput`: Structures input data for Ethereum transactions.
//! - `transaction_mined::TransactionMined`: Represents executed and mined transactions.
//! - `unix_time::UnixTime`: Manages Unix time for timestamping in Ethereum.
//! - `wei::Wei`: Manages Wei, the smallest Ether unit, for value transfers and gas calculations.
//!
//! # Ethereum Primitives: Interactions in Workflows
//!
//! This documentation focuses on the interactions among various primitives defined in the Ethereum framework,
//! particularly emphasizing how they collaboratively function in different workflows.
//!
//! ## Interactions in Key Workflows:
//!
//! ### Transaction Lifecycle
//! - `Account`, `Nonce`, `Wei`, and `Gas`: Work together to manage transaction initiation, including nonce management for replay protection, and gas calculations for transaction fees.
//! - `TransactionInput` and `ChainId`: Ensure network-specific transaction formatting and signing.
//! - `Address` and `Bytes`: Define transaction recipients and payload.
//! - `TransactionExecution`: Executes the transaction, interacting with `Gas` for consumption tracking and `Log` for event emission.
//! - `TransactionMined`: Represents the final state of a mined transaction, linking `LogMined` and block attributes (`BlockNumber`, `Hash`).
//!
//! ### Smart Contract Execution
//! - `Account` and `Slot`: Manage contract state and storage.
//! - `CallInput` and `Wei`: Facilitate contract calls with potential value transfers.
//! - `Log`, `LogTopic`, and `LogsBloom`: Record and index contract events for efficient querying.
//! - `TransactionExecution`: Handles the result of contract execution, updating state changes and emitting logs.
//!
//! ### Block Processing
//! - `Block`, `BlockHeader`, `BlockNumber`: Define the structure of a block in the blockchain.
//! - `TransactionMined` and `LogMined`: Integrate transactions and logs into the block.
//! - `Hash` and `BlockSelection`: Utilized for block identification and selection in various contexts.
//!
//! ### State Management
//! - `InMemoryValue`, `InMemoryHistory, and `StoragePointInTime`: Track changes in account states and contract storage over time.
//! - `Slot`, `Nonce`, `Wei`: Represent specific state variables like storage slots, account nonces, and balances.
//! - `LogFilter` and `LogFilterInput`: Enable the querying of historical logs based on specific criteria.
//!
//! ### Querying and Filters
//! - `LogFilter`, `LogFilterInput`, `BlockSelection`: Utilized in creating and applying filters for logs and blocks.
//! - `Log`, `LogTopic`, `LogsBloom`: Essential in filtering and retrieving event logs.
//!
//! ## Additional Interactions:
//! - `ChainId` and `TransactionInput`: Ensure network-specific processing of transactions.
//! - `SlotIndex` and `SlotValue`: Work together within contract storage for precise state management.
//! - `UnixTime`: Used across various modules for timestamping purposes.
//!
//! The outlined interactions among these primitives demonstrate the modular yet interconnected nature of the Ethereum framework. Each primitive plays a critical role in the broader context of Ethereum's operations, from individual transactions to the global state of the blockchain.

mod account;
mod address;
mod alias;
mod block;
mod block_header;
mod block_number;
mod block_selection;
mod block_size;
mod bytes;
mod call_input;
mod chain_id;
mod ecdsa_rs;
mod ecdsa_v;
mod execution;
mod execution_account_changes;
mod execution_conflict;
mod execution_result;
mod execution_value_change;
mod external_block;
mod external_receipt;
mod external_transaction;
mod gas;
mod hash;
mod index;
mod log;
mod log_filter;
mod log_filter_input;
mod log_mined;
mod log_topic;
mod logs_bloom;
mod nonce;
mod slot;
mod storage_point_in_time;
mod transaction_input;
mod transaction_mined;
mod unix_time;
mod wei;

pub use account::test_accounts;
pub use account::Account;
pub use address::Address;
pub use alias::Signature32Bytes;
pub use alias::Signature4Bytes;
pub use alias::SoliditySignature;
pub use block::Block;
pub use block_header::BlockHeader;
pub use block_number::BlockNumber;
pub use block_selection::BlockSelection;
pub use block_size::BlockSize;
pub use bytes::Bytes;
pub use call_input::CallInput;
pub use chain_id::ChainId;
pub use ecdsa_rs::EcdsaRs;
pub use ecdsa_v::EcdsaV;
pub use execution::Execution;
pub use execution::ExecutionChanges;
pub use execution_account_changes::ExecutionAccountChanges;
pub use execution_conflict::ExecutionConflict;
pub use execution_conflict::ExecutionConflicts;
pub use execution_conflict::ExecutionConflictsBuilder;
pub use execution_result::ExecutionResult;
pub use execution_value_change::ExecutionValueChange;
pub use external_block::ExternalBlock;
pub use external_receipt::ExternalReceipt;
pub use external_transaction::ExternalTransaction;
pub use external_transaction::ExternalTransactionExecution;
pub use gas::Gas;
pub use hash::Hash;
pub use index::Index;
pub use log::Log;
pub use log_filter::LogFilter;
pub use log_filter::LogFilterTopicCombination;
pub use log_filter_input::LogFilterInput;
pub use log_mined::LogMined;
pub use log_topic::LogTopic;
pub use nonce::Nonce;
pub use slot::Slot;
pub use slot::SlotIndex;
pub use slot::SlotValue;
pub use storage_point_in_time::StoragePointInTime;
pub use transaction_input::TransactionInput;
pub use transaction_mined::TransactionMined;
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
