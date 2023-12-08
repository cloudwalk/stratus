//! EVM bytecode executor

#[allow(clippy::module_inception)]
mod evm;
pub mod revm;

pub use evm::Evm;
pub use evm::EvmInput;
