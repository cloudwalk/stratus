//! EVM bytecode executor

#[allow(clippy::module_inception)]
mod evm;
pub mod revm;

pub use evm::Evm;
pub use evm::EvmCall;
pub use evm::EvmDeployment;
pub use evm::EvmTransaction;
