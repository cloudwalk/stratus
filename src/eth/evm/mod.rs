//! EVM bytecode executor

#[allow(clippy::module_inception)]
mod evm;
mod evm_storage;
pub mod revm;

pub use evm::Evm;
pub use evm::EvmCall;
pub use evm::EvmDeployment;
pub use evm::EvmTransaction;
pub use evm_storage::EvmStorage;
