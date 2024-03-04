//! Ethereum Virtual Machine (EVM) Module
//!
//! This module serves as the backbone for executing Ethereum transactions in the Stratus project.
//! It abstracts the EVM logic through the `Evm` trait and provides `EvmInput` for parameter encapsulation.
//! Implementations of this trait, like `Revm`, offer concrete execution logic, leveraging dynamic dispatch
//! via `Box<dyn Evm>` to allow for flexible, runtime-determined EVM operations.

#[allow(clippy::module_inception)]
mod evm;
pub mod revm;

pub use evm::Evm;
pub use evm::EvmExecutionResult;
pub use evm::EvmInput;
