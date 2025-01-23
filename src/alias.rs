//! Type aliases for external crates types that conflict with our own types or are too verbose.

use ethereum_types::H256;

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
pub type AlloyBlockAlloyExternalTransaction = alloy_rpc_types_eth::Block<ExternalTransaction>;
pub type AlloyBlockH256 = alloy_rpc_types_eth::Block<H256>;
pub type AlloyBytes = alloy_primitives::Bytes;
pub type AlloyLog = alloy_rpc_types_eth::Log;
pub type AlloyBloom = alloy_primitives::Bloom;
pub type AlloyReceipt = alloy_rpc_types_eth::TransactionReceipt;
pub type AlloyTransaction = alloy_rpc_types_eth::Transaction;

// -----------------------------------------------------------------------------
// Ethers
// -----------------------------------------------------------------------------
pub type EthersBlockVoid = ethers_core::types::Block<()>;
pub type EthersBlockEthersTransaction = ethers_core::types::Block<ethers_core::types::Transaction>;
pub type EthersBlockExternalTransaction = ethers_core::types::Block<ExternalTransaction>;
pub type EthersBlockH256 = ethers_core::types::Block<H256>;
pub type EthersBytes = ethers_core::types::Bytes;
pub type EthersReceipt = ethers_core::types::TransactionReceipt;
pub type EthersTransaction = ethers_core::types::Transaction;

// -----------------------------------------------------------------------------
// REVM
// -----------------------------------------------------------------------------
pub type RevmAccountInfo = revm::primitives::AccountInfo;
pub type RevmAddress = revm::primitives::Address;
pub type RevmB256 = revm::primitives::B256;
pub type RevmBytecode = revm::primitives::Bytecode;
pub type RevmBytes = revm::primitives::Bytes;
pub type RevmLog = revm::primitives::Log;
pub type RevmOutput = revm::primitives::Output;
pub type RevmState = revm::primitives::EvmState;
pub type RevmU256 = revm::primitives::U256;
