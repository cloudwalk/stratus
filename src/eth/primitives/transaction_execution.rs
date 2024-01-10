use std::collections::HashMap;
use std::fmt::Debug;

use itertools::Itertools;
use revm::primitives::ExecutionResult as RevmExecutionResult;
use revm::primitives::ResultAndState as RevmResultAndState;
use revm::primitives::State as RevmState;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;
use crate::eth::EthError;
use crate::ext::not;

pub type ExecutionChanges = HashMap<Address, TransactionExecutionAccountChanges>;

// -----------------------------------------------------------------------------
// Transaction Execution Result
// -----------------------------------------------------------------------------

/// Indicates how a transaction was finished.
#[derive(Debug, strum::Display, Clone, PartialEq, Eq, fake::Dummy, derive_new::new, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionExecutionResult {
    /// Transaction execution finished normally (RETURN).
    #[strum(serialize = "success")]
    Success,

    /// Transaction execution finished with a reversion (REVERT).
    #[strum(serialize = "reverted")]
    Reverted,

    /// Transaction execution did not finish.
    #[strum(serialize = "halted")]
    Halted { reason: String },
}

// -----------------------------------------------------------------------------
// Transaction Execution
// -----------------------------------------------------------------------------
/// Output of a executed transaction in the EVM.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionExecution {
    /// Status of the execution.
    pub result: TransactionExecutionResult,

    /// Output returned by the function execution (can be the function output or an exeception).
    pub output: Bytes,

    /// Logs emitted by the function execution.
    pub logs: Vec<Log>,

    /// Consumed gas.
    pub gas: Gas,

    /// Assumed block timestamp during the execution.
    pub block_timestamp_in_secs: u64,

    /// Storage changes that happened during the transaction execution.
    pub changes: Vec<TransactionExecutionAccountChanges>,
}

impl TransactionExecution {
    /// Apply REVM transaction execution result to the storage original values, creating a new `TransactionExecution` that can be used to update the state.
    ///
    /// TODO: move this function to REVM submodule.
    pub fn from_revm_result(
        revm_result: RevmResultAndState,
        execution_block_timestamp_in_secs: u64,
        execution_changes: ExecutionChanges,
    ) -> Result<Self, EthError> {
        let (result, output, logs, gas) = parse_revm_result(revm_result.result);
        let execution_changes = parse_revm_state(revm_result.state, execution_changes)?;

        tracing::info!(%result, %gas, output_len = %output.len(), %output, "evm executed");
        Ok(Self {
            result,
            output,
            logs,
            gas,
            block_timestamp_in_secs: execution_block_timestamp_in_secs,
            changes: execution_changes.into_values().collect(),
        })
    }

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
        matches!(self.result, TransactionExecutionResult::Success { .. })
    }
}

/// TODO: move this function to REVM submodule.
fn parse_revm_result(result: RevmExecutionResult) -> (TransactionExecutionResult, Bytes, Vec<Log>, Gas) {
    match result {
        RevmExecutionResult::Success { output, gas_used, logs, .. } => {
            let result = TransactionExecutionResult::Success;
            let output = Bytes::from(output);
            let logs = logs.into_iter().map_into().collect();
            let gas = Gas::from(gas_used);
            (result, output, logs, gas)
        }
        RevmExecutionResult::Revert { output, gas_used } => {
            let result = TransactionExecutionResult::Reverted;
            let output = Bytes::from(output);
            let gas = Gas::from(gas_used);
            (result, output, Vec::new(), gas)
        }
        RevmExecutionResult::Halt { reason, gas_used } => {
            let result = TransactionExecutionResult::new_halted(format!("{:?}", reason));
            let output = Bytes::default();
            let gas = Gas::from(gas_used);
            (result, output, Vec::new(), gas)
        }
    }
}

/// TODO: move this function to REVM submodule.
fn parse_revm_state(revm_state: RevmState, mut execution_changes: ExecutionChanges) -> Result<ExecutionChanges, EthError> {
    for (revm_address, revm_account) in revm_state {
        let address: Address = revm_address.into();

        // do not apply state changes to coinbase because we do not charge gas, otherwise it will have to be updated for every transaction
        if address.is_coinbase() {
            continue;
        }

        // apply changes according to account status
        tracing::debug!(%address, status = ?revm_account.status, slots = %revm_account.storage.len(), "evm account");
        let (account_created, account_updated) = (revm_account.is_created(), revm_account.is_touched());

        // parse revm to internal representation
        let account: Account = (revm_address, revm_account.info).into();
        let account_modified_slots: Vec<Slot> = revm_account
            .storage
            .into_iter()
            .map(|(index, value)| Slot::new(index, value.present_value))
            .collect();

        // status: created
        if account_created {
            execution_changes.insert(
                account.address.clone(),
                TransactionExecutionAccountChanges::from_created_account(account, account_modified_slots),
            );
        }
        // status: touched (updated)
        else if account_updated {
            let Some(existing_account) = execution_changes.get_mut(&address) else {
                tracing::error!(keys = ?execution_changes.keys(), reason = "account was updated, but it was not loaded by evm", %address);
                return Err(EthError::AccountNotLoaded(address));
            };
            existing_account.apply_changes(account, account_modified_slots);
        }
    }
    Ok(execution_changes)
}

// -----------------------------------------------------------------------------
// Account Changes
// -----------------------------------------------------------------------------

/// Changes that happened to an account during a transaction.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionExecutionAccountChanges {
    pub address: Address,
    pub nonce: TransactionExecutionValueChange<Nonce>,
    pub balance: TransactionExecutionValueChange<Wei>,
    pub bytecode: TransactionExecutionValueChange<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, TransactionExecutionValueChange<Slot>>,
}

impl TransactionExecutionAccountChanges {
    /// Create a new `TransactionAccountChanges` that represents an existing account from the storage.
    pub fn from_existing_account(account: impl Into<Account>) -> Self {
        let account = account.into();
        Self {
            address: account.address,
            nonce: TransactionExecutionValueChange::from_original(account.nonce),
            balance: TransactionExecutionValueChange::from_original(account.balance),
            bytecode: TransactionExecutionValueChange::from_original(account.bytecode),
            slots: HashMap::new(),
        }
    }

    /// Create a new `TransactionAccountChanges` that represents a new account being created and that does not exist in the storage.
    pub fn from_created_account(account: Account, modified_slots: Vec<Slot>) -> Self {
        let mut changes = Self {
            address: account.address,
            nonce: TransactionExecutionValueChange::from_modified(account.nonce),
            balance: TransactionExecutionValueChange::from_modified(account.balance),
            bytecode: TransactionExecutionValueChange::from_modified(account.bytecode),
            slots: HashMap::new(),
        };

        for slot in modified_slots {
            changes.slots.insert(slot.index.clone(), TransactionExecutionValueChange::from_modified(slot));
        }

        changes
    }

    /// Updates an existing account with the changes that happened during the transaction.
    pub fn apply_changes(&mut self, account: Account, modified_slots: Vec<Slot>) {
        self.nonce.set_modified(account.nonce);
        self.balance.set_modified(account.balance);

        for slot in modified_slots {
            match self.slots.get_mut(&slot.index) {
                Some(ref mut entry) => {
                    entry.set_modified(slot);
                }
                None => {
                    self.slots.insert(slot.index.clone(), TransactionExecutionValueChange::from_modified(slot));
                }
            };
        }
    }
}

// -----------------------------------------------------------------------------
// Value Changes
// -----------------------------------------------------------------------------

/// Changes that happened to an account value during a transaction.
///
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionExecutionValueChange<T>
where
    T: PartialEq,
{
    pub original: TransactionExecutionValueChangeState<T>,
    pub modified: TransactionExecutionValueChangeState<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionExecutionValueChangeState<T> {
    Set(T),
    NotSet,
}

impl<T> TransactionExecutionValueChangeState<T> {
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub fn is_not_set(&self) -> bool {
        not(self.is_set())
    }
}

impl<T> TransactionExecutionValueChange<T>
where
    T: PartialEq,
{
    /// Create a new `TransactionValueChange` only with original value.
    pub fn from_original(value: T) -> Self {
        Self {
            original: TransactionExecutionValueChangeState::Set(value),
            modified: TransactionExecutionValueChangeState::NotSet,
        }
    }

    /// Create a new `TransactionValueChange` only with modified value.
    pub fn from_modified(value: T) -> Self {
        Self {
            original: TransactionExecutionValueChangeState::NotSet,
            modified: TransactionExecutionValueChangeState::Set(value),
        }
    }

    /// Set the modified value of an original value.
    pub fn set_modified(&mut self, value: T) {
        if self.original.is_not_set() {
            tracing::warn!("Setting modified value without original value. Use `new_modified` instead.");
        }
        self.modified = TransactionExecutionValueChangeState::Set(value);
    }

    /// Take the modified value only if the value was modified.
    pub fn take_if_modified(self) -> Option<T> {
        if let TransactionExecutionValueChangeState::Set(value) = self.modified {
            Some(value)
        } else {
            None
        }
    }

    /// Check if the value was modified.
    pub fn is_modified(&self) -> bool {
        self.modified.is_set() && (self.original != self.modified)
    }
}
