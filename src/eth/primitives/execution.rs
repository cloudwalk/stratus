//! Transaction Execution Module
//!
//! Handles the details and results of executing transactions in the Ethereum
//! Virtual Machine (EVM). This module defines structures to represent
//! transaction outcomes, including status, gas usage, logs, and state changes.
//! It is vital for interpreting the results of transaction execution and
//! applying changes to the Ethereum state.

use std::collections::HashMap;
use std::fmt::Debug;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;

pub type ExecutionChanges = HashMap<Address, ExecutionAccountChanges>;

/// Output of a executed transaction in the EVM.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Execution {
    /// Status of the execution.
    pub result: ExecutionResult,

    /// Output returned by the function execution (can be the function output or an exeception).
    pub output: Bytes,

    /// Logs emitted by the function execution.
    pub logs: Vec<Log>,

    /// Consumed gas.
    pub gas: Gas,

    /// Assumed block timestamp during the execution.
    // TODO: use UnixTime type
    pub block_timestamp_in_secs: u64,

    /// Storage changes that happened during the transaction execution.
    pub changes: Vec<ExecutionAccountChanges>,
}

impl Execution {
    /// When the transaction is a contract deployment, returns the address of the deployed contract.
    pub fn contract_address(&self) -> Option<Address> {
        for changes in &self.changes {
            if changes.bytecode.is_modified() {
                return Some(changes.address.clone());
            }
        }
        None
    }

    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        matches!(self.result, ExecutionResult::Success { .. })
    }
}
