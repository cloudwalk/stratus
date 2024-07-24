//! Type aliases for external crates types that conflict with our own types or are too verbose.

// -----------------------------------------------------------------------------
// Serde
// -----------------------------------------------------------------------------
pub type JsonValue = serde_json::Value;

// -----------------------------------------------------------------------------
// Ethers
// -----------------------------------------------------------------------------
pub type EthersBytes = ethers_core::types::Bytes;
pub type EthersLog = ethers_core::types::Log;
pub type EthersReceipt = ethers_core::types::TransactionReceipt;
pub type EthersTransaction = ethers_core::types::Transaction;

// -----------------------------------------------------------------------------
// REVM
// -----------------------------------------------------------------------------
pub type RevmAddress = revm::primitives::Address;
pub type RevmBytecode = revm::primitives::Bytecode;
pub type RevmAccountInfo = revm::primitives::AccountInfo;
pub type RevmState = revm::primitives::State;
pub type RevmU256 = revm::primitives::U256;
pub type RevmB256 = revm::primitives::B256;
pub type RevmLog = revm::primitives::Log;
pub type RevmBytes = revm::primitives::Bytes;
pub type RevmOutput = revm::primitives::Output;
