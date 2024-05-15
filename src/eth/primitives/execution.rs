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
use anyhow::Ok;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::log_and_err;

pub type ExecutionChanges = HashMap<Address, ExecutionAccountChanges>;

/// Output of a transaction executed in the EVM.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct EvmExecution {
    /// Assumed block timestamp during the execution.
    pub block_timestamp: UnixTime,

    /// Flag to indicate if  external receipt fixes have been applied.
    pub receipt_applied: bool,

    /// Status of the execution.
    pub result: ExecutionResult,

    /// Output returned by the function execution (can be the function output or an exeception).
    pub output: Bytes,

    /// Logs emitted by the function execution.
    pub logs: Vec<Log>,

    /// Consumed gas.
    pub gas: Gas,

    /// Storage changes that happened during the transaction execution.
    pub changes: HashMap<Address, ExecutionAccountChanges>,

    /// The contract address if the executed transaction deploys a contract.
    pub deployed_contract_address: Option<Address>,
}

impl EvmExecution {
    /// Creates an execution from an external transaction that failed.
    pub fn from_failed_external_transaction(sender: Account, receipt: &ExternalReceipt, block: &ExternalBlock) -> anyhow::Result<Self> {
        if receipt.is_success() {
            return log_and_err!("cannot create failed execution for successful transaction");
        }
        if not(receipt.logs.is_empty()) {
            return log_and_err!("failed receipt should not have produced logs");
        }

        // generate sender changes incrementing the nonce
        let mut sender_changes = ExecutionAccountChanges::from_original_values(sender);
        let sender_next_nonce = sender_changes.nonce.take_original_ref().unwrap().next();
        sender_changes.nonce.set_modified(sender_next_nonce);

        // crete execution and apply costs
        let mut execution = Self {
            block_timestamp: block.timestamp(),
            receipt_applied: false,
            result: ExecutionResult::new_reverted(), // assume it reverted
            output: Bytes::default(),                // we cannot really know without performing an eth_call to the external system
            logs: Vec::new(),
            gas: receipt.gas_used.unwrap_or_default().try_into()?,
            changes: HashMap::from([(sender_changes.address, sender_changes)]),
            deployed_contract_address: None,
        };
        execution.apply_receipt(receipt)?;
        Ok(execution)
    }

    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        matches!(self.result, ExecutionResult::Success { .. })
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        not(self.is_success())
    }

    /// Returns the address of the deployed contract if the transaction is a deployment.
    pub fn contract_address(&self) -> Option<Address> {
        if let Some(contract_address) = &self.deployed_contract_address {
            return Some(contract_address.to_owned());
        }

        for changes in self.changes.values() {
            if changes.bytecode.is_modified() {
                return Some(changes.address);
            }
        }
        None
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
            tracing::trace!("execution logs: {:#?}", self.logs);
            tracing::trace!("receipt logs: {:#?}", receipt.logs);
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
            if execution_log.topics().len() != receipt_log.topics.len() {
                return log_and_err!(format!(
                    "log topics length mismatch | hash={} log_index={} execution={} receipt={}",
                    receipt.hash(),
                    log_index,
                    execution_log.topics().len(),
                    receipt_log.topics.len(),
                ));
            }

            // compare log topics content
            for (topic_index, (execution_log_topic, receipt_log_topic)) in execution_log.topics().iter().zip(&receipt_log.topics).enumerate() {
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

    /// External transactions are re-executed locally with max gas and zero gas price.
    ///
    /// This causes some attributes to be different from the original execution.
    ///
    /// This method updates the attributes that can diverge based on the receipt of the external transaction.
    pub fn apply_receipt(&mut self, receipt: &ExternalReceipt) -> anyhow::Result<()> {
        // do nothing if receipt is already applied
        if self.receipt_applied {
            return Ok(());
        }
        self.receipt_applied = true;

        // fix gas
        self.gas = receipt.gas_used.unwrap_or_default().try_into()?;

        // TODO: fix logs

        // fix sender balance
        let execution_cost = receipt.execution_cost();
        if execution_cost > Wei::ZERO {
            // find sender changes
            let sender_address: Address = receipt.0.from.into();
            let Some(sender_changes) = self.changes.get_mut(&sender_address) else {
                return log_and_err!("sender changes not present in execution when applying execution costs");
            };

            // subtract execution cost from sender balance
            let sender_balance = *sender_changes.balance.take_ref().expect("balance is never None");
            let sender_new_balance = if sender_balance > execution_cost {
                sender_balance - execution_cost
            } else {
                Wei::ZERO
            };
            sender_changes.balance.set_modified(sender_new_balance);
        }

        Ok(())
    }
}
