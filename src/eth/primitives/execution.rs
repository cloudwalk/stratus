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
use ethereum_types::H256;
use ethereum_types::U64;

use super::ExternalReceipt;
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

    pub fn cmp_with_receipt(&self, receipt: &ExternalReceipt) -> anyhow::Result<()> {
        // Should the gas be the same?
        // assert_eq!(execution.gas, receipt.gas_used.unwrap_or_default().into());

        let exec_result: U64 = match self.result {
            ExecutionResult::Success => 1,
            _ => 0,
        }
        .into();
        let rcpt_status = receipt.status.unwrap_or_default();
        if exec_result != rcpt_status {
            return Err(anyhow!("tx result mismatch, expected: {:?} got: {:?}", rcpt_status, exec_result));
        }

        if self.logs.len() != receipt.logs.len() {
            return Err(anyhow!(
                "tx logs length mismatch, expected: {:?} got: {:?}",
                receipt.logs.len(),
                self.logs.len()
            ));
        }

        for (log, external_log) in self.logs.iter().zip(&receipt.logs) {
            if log.topics.len() != external_log.topics.len() {
                return Err(anyhow!(
                    "tx topics length mismatch, expected: {:?} got: {:?}",
                    external_log.topics.len(),
                    log.topics.len()
                ));
            }
            if log.data.as_ref() != external_log.data.as_ref() {
                return Err(anyhow!(
                    "tx log data mismatch, expected: {:?} got: {:?}",
                    external_log.data.as_ref(),
                    log.data.as_ref()
                ));
            }
            for (topic, external_topic) in log.topics.iter().zip(&external_log.topics) {
                let topic: H256 = topic.to_owned().into();
                let external_topic = external_topic.to_owned();
                if topic != external_topic {
                    return Err(anyhow!("tx topic mismatch, expected: {:?} got: {:?}", external_topic, topic));
                }
            }
        }
        Ok(())
    }
}
