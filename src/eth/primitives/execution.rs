//! Transaction Execution Module
//!
//! Handles the details and results of executing transactions in the Ethereum
//! Virtual Machine (EVM). This module defines structures to represent
//! transaction outcomes, including status, gas usage, logs, and state changes.
//! It is vital for interpreting the results of transaction execution and
//! applying changes to the Ethereum state.

use std::collections::HashMap;
use std::fmt::Debug;

use anyhow::anyhow;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::UnixTime;
use crate::log_and_err;

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
    pub block_timestamp: UnixTime,

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

    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        matches!(self.result, ExecutionResult::Success { .. })
    }

    /// Checks if current execution state matches the information present in the external receipt.
    pub fn compare_with_receipt(&self, receipt: &ExternalReceipt) -> anyhow::Result<()> {
        // compare execution status
        if self.is_success() != receipt.is_success() {
            return log_and_err!(format!(
                "transaction status mismatch | hash={} execution={:?} receipt={:?}",
                receipt.hash(),
                self.result,
                receipt.status
            ));
        }

        // compare logs length
        if self.logs.len() != receipt.logs.len() {
            return log_and_err!(format!(
                "logs length mismatch | hash={} execution={} receipt={}",
                receipt.hash(),
                self.logs.len(),
                receipt.logs.len()
            ));
        }

        // compare logs pairs
        for (log_index, (execution_log, receipt_log)) in self.logs.iter().zip(&receipt.logs).enumerate() {
            // compare log topics length
            if execution_log.topics.len() != receipt_log.topics.len() {
                return log_and_err!(format!(
                    "log topics length mismatch | hash={} log_index={} execution={} receipt={}",
                    receipt.hash(),
                    log_index,
                    execution_log.topics.len(),
                    receipt_log.topics.len(),
                ));
            }

            // compare log topics content
            for (topic_index, (execution_log_topic, receipt_log_topic)) in execution_log.topics.iter().zip(&receipt_log.topics).enumerate() {
                if execution_log_topic.as_ref() != receipt_log_topic.as_ref() {
                    return log_and_err!(format!(
                        "log topic content mismatch | hash={} log_index={} topic_index={} execution={} receipt={:#x}",
                        receipt.hash(),
                        log_index,
                        topic_index,
                        execution_log_topic,
                        receipt_log_topic,
                    ));
                }
            }

            // compare log data content
            if execution_log.data.as_ref() != receipt_log.data.as_ref() {
                return log_and_err!(format!(
                    "log data content mismatch | hash={} log_index={} execution={} receipt={:#x}",
                    receipt.hash(),
                    log_index,
                    execution_log.data,
                    receipt_log.data,
                ));
            }
        }
        Ok(())
    }

    /// Apply execution costs of an external transaction.
    ///
    /// External transactions are re-executed locally with max gas and zero gas price,
    /// so the paid amount is applied after execution based on the receipt.
    pub fn apply_execution_costs(&mut self, receipt: &ExternalReceipt) -> anyhow::Result<()> {
        // do nothing if execution cost is zero
        let execution_cost = receipt.execution_cost();
        if execution_cost.is_zero() {
            return Ok(());
        }

        // find sender changes (this can be improved if changes is HashMap)
        let sender_address: Address = receipt.0.from.into();
        let sender_changes = self.changes.iter_mut().find(|c| c.address == sender_address);
        let Some(sender_changes) = sender_changes else {
            return log_and_err!("sender changes not present in execution when applying execution costs");
        };

        // subtract execution cost from sender balance
        let current_balance = sender_changes.balance.take_ref().expect("balance is never None").clone();
        let new_balance = current_balance - execution_cost; // TODO: handle underflow, but it should not happen
        sender_changes.balance.set_modified(new_balance);
        Ok(())
    }
}
