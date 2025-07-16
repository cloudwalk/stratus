//! Type aliases for external crates types that conflict with our own types or are too verbose.

use alloy_primitives::B256;

use crate::eth::primitives::ExternalTransaction;

// -----------------------------------------------------------------------------
// Serde
// -----------------------------------------------------------------------------
pub type JsonValue = serde_json::Value;

// -----------------------------------------------------------------------------
// Alloy
// -----------------------------------------------------------------------------
pub type AlloyBlockVoid = alloy_rpc_types_eth::Block<()>;
pub type AlloyBlockAlloyTransaction = alloy_rpc_types_eth::Block<alloy_rpc_types_eth::Transaction>;
pub type AlloyBlockExternalTransaction = alloy_rpc_types_eth::Block<ExternalTransaction>;
pub type AlloyBlockB256 = alloy_rpc_types_eth::Block<B256>;
pub type AlloyBytes = alloy_primitives::Bytes;
pub type AlloyLog = alloy_rpc_types_eth::Log;
pub type AlloyLogData = alloy_primitives::LogData;
pub type AlloyLogPrimitive = alloy_primitives::Log;
pub type AlloyBloom = alloy_primitives::Bloom;
pub type AlloyReceipt = alloy_rpc_types_eth::TransactionReceipt;
pub type AlloyTransaction = alloy_rpc_types_eth::Transaction;
pub type AlloyAddress = alloy_primitives::Address;
pub type AlloyConsensusHeader = alloy_consensus::Header;
pub type AlloyHeader = alloy_rpc_types_eth::Header;

// -----------------------------------------------------------------------------
// REVM
// -----------------------------------------------------------------------------
pub type RevmAccountInfo = revm::state::AccountInfo;
pub type RevmAddress = revm::primitives::Address;
pub type RevmBytecode = revm::state::Bytecode;
pub type RevmBytes = revm::primitives::Bytes;
pub type RevmLog = revm::primitives::Log;
pub type RevmOutput = revm::context::result::Output;
pub type RevmState = revm::state::EvmState;
