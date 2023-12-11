use std::collections::HashMap;
use std::fmt::Debug;

use revm::primitives::ExecutionResult as RevmExecutionResult;
use revm::primitives::ResultAndState as RevmResultAndState;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Amount;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::EthError;

// -----------------------------------------------------------------------------
// Transaction Execution Result
// -----------------------------------------------------------------------------

/// Indicates how a transaction was finished.
#[derive(Debug, Clone, strum::Display)]
pub enum TransactionExecutionResult {
    /// Transaction execution finished normally (RETURN).
    #[strum(serialize = "Commited")]
    Commited,

    /// Transaction execution finished with a reversion (REVERT).
    #[strum(serialize = "Reverted")]
    Reverted,

    /// Transaction execution did not finish.
    #[strum(serialize = "Halted")]
    Halted { reason: String },
}

// -----------------------------------------------------------------------------
// Transaction Execution
// -----------------------------------------------------------------------------
/// Output of a executed transaction in the EVM.
#[derive(Debug, Clone)]
pub struct TransactionExecution {
    pub result: TransactionExecutionResult,
    pub output: Bytes,
    pub gas_used: Gas,
    pub changes: Vec<AccountChanges>,
}

impl TransactionExecution {
    /// Apply REVM transaction execution result to the storage original values, creating a new `TransactionExecution` that can be used to update the state.
    pub fn from_revm_result(revm_result: RevmResultAndState, mut storage_changes: HashMap<Address, AccountChanges>) -> Result<Self, EthError> {
        // parse result
        let (result, output, gas) = match revm_result.result {
            RevmExecutionResult::Success { output, gas_used, .. } => (TransactionExecutionResult::Commited, Bytes::from(output), gas_used),
            RevmExecutionResult::Revert { output, gas_used } => (TransactionExecutionResult::Reverted, Bytes::from(output), gas_used),
            RevmExecutionResult::Halt { reason, gas_used } => (
                TransactionExecutionResult::Halted {
                    reason: format!("{:?}", reason),
                },
                Bytes::default(),
                gas_used,
            ),
        };

        // parse state changes
        for (revm_address, revm_account) in revm_result.state {
            // ignore coinbase address changes because we don't charge gas
            let address: Address = revm_address.into();
            if address.is_coinbase() {
                continue;
            }

            // apply changes according to account status
            tracing::debug!(status = ?revm_account.status, %address,  "applying account changes");

            // parse revm status
            let account_created = revm_account.is_created();
            let account_updated = revm_account.is_touched();

            // parse revm to internal representation
            let account: Account = (revm_address, revm_account.info).into();
            let account_modified_slots: Vec<Slot> = revm_account
                .storage
                .into_iter()
                .map(|(index, value)| Slot::new(index, value.present_value))
                .collect();

            // status: created
            if account_created {
                storage_changes.insert(account.address.clone(), AccountChanges::from_created_account(account, account_modified_slots));
            }
            // status: touched (updated)
            else if account_updated {
                let existing_account = match storage_changes.get_mut(&address) {
                    Some(account) => account,
                    None => {
                        tracing::error!(keys = ?storage_changes.keys(), reason = "account updated, but account was not loaded", %address);
                        return Err(EthError::AccountNotLoaded(address));
                    }
                };
                existing_account.apply_changes(account, account_modified_slots);
            }
        }

        if output.len() > 256 {
            tracing::info!(%result, %gas, output_len = %output.len(), output = %"too long", "executed");
        } else {
            tracing::info!(%result, %gas, output_len = %output.len(), %output, "executed");
        }
        Ok(Self {
            result,
            output,
            gas_used: gas.into(),
            changes: storage_changes.into_values().collect(),
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
}

// -----------------------------------------------------------------------------
// Account Changes
// -----------------------------------------------------------------------------

/// Changes that happened to an account during a transaction.
#[derive(Debug, Clone)]
pub struct AccountChanges {
    pub address: Address,
    pub nonce: ValueChange<Nonce>,
    pub balance: ValueChange<Amount>,
    pub bytecode: ValueChange<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, ValueChange<Slot>>,
}

impl AccountChanges {
    /// Create a new `TransactionAccountChanges` that represents an existing account from the storage.
    pub fn from_existing_account(account: impl Into<Account>) -> Self {
        let account = account.into();
        Self {
            address: account.address,
            nonce: ValueChange::from_original(account.nonce),
            balance: ValueChange::from_original(account.balance),
            bytecode: ValueChange::from_original(account.bytecode),
            slots: HashMap::new(),
        }
    }

    /// Create a new `TransactionAccountChanges` that represents a new account being created and that does not exist in the storage.
    pub fn from_created_account(account: Account, modified_slots: Vec<Slot>) -> Self {
        let mut changes = Self {
            address: account.address,
            nonce: ValueChange::from_modified(account.nonce),
            balance: ValueChange::from_modified(account.balance),
            bytecode: ValueChange::from_modified(account.bytecode),
            slots: HashMap::new(),
        };

        for slot in modified_slots {
            changes.slots.insert(slot.index.clone(), ValueChange::from_modified(slot));
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
                    entry.modified = Some(slot);
                }
                None => {
                    self.slots.insert(slot.index.clone(), ValueChange::from_modified(slot));
                }
            };
        }
    }
}

// -----------------------------------------------------------------------------
// Value Changes
// -----------------------------------------------------------------------------

/// Changes that happened to an account value during a transaction.
#[derive(Debug, Clone)]
pub struct ValueChange<T>
where
    T: PartialEq,
{
    pub original: Option<T>,
    pub modified: Option<T>,
}

impl<T> ValueChange<T>
where
    T: PartialEq,
{
    /// Create a new `TransactionValueChange` only with original value.
    pub fn from_original(value: T) -> Self {
        Self {
            original: Some(value),
            modified: None,
        }
    }

    /// Create a new `TransactionValueChange` only with modified value.
    pub fn from_modified(value: T) -> Self {
        Self {
            original: None,
            modified: Some(value),
        }
    }

    /// Set the modified value of an original value.
    pub fn set_modified(&mut self, value: T) {
        if self.original.is_none() {
            tracing::warn!("Setting modified value without original value. Use `new_modified` instead.");
        }
        self.modified = Some(value);
    }

    /// Take the modified value only if the value was modified.
    pub fn take_if_modified(&mut self) -> Option<T> {
        if self.is_modified() {
            self.modified.take()
        } else {
            None
        }
    }

    /// Check if the value was modified.
    pub fn is_modified(&self) -> bool {
        self.modified.is_some() && (self.original != self.modified)
    }
}
