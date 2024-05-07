//! Ethereum components.
//! ----------------------------
//!
//! The 'eth' directory encapsulates the Ethereum blockchain operations within the Stratus project. It includes modules for transaction processing, EVM emulation, block mining, and state management, as well as interfaces for RPC communication.
//!
//! Modules detail:
//! - codegen: Generates code required for Ethereum operations.
//! - evm: Core of the Ethereum Virtual Machine implementation.
//! - executor: Orchestrates the transaction execution process.
//! - miner: Responsible for creating new blocks in the blockchain.
//! - primitives: Basic data structures such as Blocks and Transactions.
//! - rpc: Handles communication with Ethereum nodes via RPC.
//! - storage: Stores and retrieves blockchain data, ensuring data integrity and access.
//!
//!
//! A key feature of this module is the `executor` submodule, specifically its `transact` function.
//! This function represents a significant shift from traditional blockchain models by processing
//! transactions and creating blocks based on transaction activity rather than fixed time intervals.
//! This approach allows for a more dynamic and responsive blockchain, adapting quickly to varying
//! transaction loads and ensuring timely block generation.

mod block_miner;
pub mod codegen;
pub mod evm;
mod executor;
pub mod primitives;
pub mod rpc;
pub mod storage;
pub mod transaction_relayer;

pub use block_miner::BlockMiner;
pub use executor::EvmTask;
pub use executor::Executor;
pub use transaction_relayer::TransactionRelayer;
