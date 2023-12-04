use std::collections::HashMap;

use revm::primitives::ExecutionResult;
use revm::primitives::ResultAndState;

use super::Slot;
use crate::evm::entities::Address;
use crate::evm::entities::Bytecode;
use crate::evm::entities::Nonce;

/// Output of a executed transaction in the EVM.
#[derive(Debug, Clone, Default)]
pub struct TransactionExecution {
    pub changes: HashMap<Address, TransactionExecutionChange>,
}

impl TransactionExecution {
    /// When the transaction is a contract deployment, returns the address of the deployed contract.
    pub fn deployment_address(&self) -> Option<Address> {
        for (address, changes) in &self.changes {
            if changes.bytecode.is_some() {
                return Some(address.clone());
            }
        }
        None
    }
}

impl From<ResultAndState> for TransactionExecution {
    fn from(result: ResultAndState) -> Self {
        let mut execution = Self::default();

        // parse result
        let (reason, output) = match result.result {
            ExecutionResult::Success { reason, output, .. } => (format!("Success: {:?}", reason), const_hex::encode(output.data())),
            ExecutionResult::Revert { output, .. } => ("Revert".to_owned(), const_hex::encode(output)),
            ExecutionResult::Halt { reason, .. } => (format!("Halt: {:?}", reason), "".to_owned()),
        };
        tracing::info!(%reason, %output, "executed");

        // parse changes
        for (revm_address, revm_changes) in result.state {
            // ignore accounts that where not touched
            if !revm_changes.is_touched() {
                continue;
            }
            // ignore changes to coinbase address because we don't charge gas
            let address: Address = revm_address.into();
            if address.is_coinbase() {
                continue;
            }

            let account_changes = execution.changes.entry(revm_address.into()).or_default();

            // nonce
            account_changes.nonce = revm_changes.info.nonce.into();

            // bytecode (set only when account was created)
            if revm_changes.is_created() {
                account_changes.bytecode = revm_changes.info.code.map(|x| x.into());
            }

            // slots
            for revm_slot in revm_changes.storage {
                account_changes.slots.push(revm_slot.into());
            }
        }

        execution
    }
}

#[derive(Debug, Clone, Default)]
pub struct TransactionExecutionChange {
    pub nonce: Nonce,
    pub bytecode: Option<Bytecode>,
    pub slots: Vec<Slot>,
}
