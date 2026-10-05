use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::ensure;

use crate::alias::RevmBytecode;
use crate::eth::executor::types::state::AccountChanges;
use crate::eth::executor::types::state::AccountOriginalsReader;
use crate::eth::executor::types::state::Change;
use crate::eth::executor::types::state::Complete;
use crate::eth::executor::types::state::CompleteValue;
use crate::eth::executor::types::state::Final;
use crate::eth::executor::types::state::State;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;

/// Shared permanent-state baseline for completion and replay comparison.
pub(super) struct ReplayPrestate {
    pub accounts: HashMap<Address, Account>,
    pub slots: HashMap<(Address, SlotIndex), SlotValue>,
}

impl ReplayPrestate {
    pub fn account(&self, address: Address) -> Account {
        self.accounts.get(&address).cloned().unwrap_or_else(|| Account::new_empty(address))
    }

    pub fn slot(&self, address: Address, index: SlotIndex) -> SlotValue {
        self.slots.get(&(address, index)).copied().unwrap_or_default()
    }
}

impl AccountOriginalsReader for ReplayPrestate {
    fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>> {
        Ok(addresses.into_iter().map(|address| (address, self.account(address))).collect())
    }
}

/// Validates original payloads against permanent prestate before committing them to the cache.
pub(super) fn validate_original_values(
    actual: &State<Complete>,
    mut read_account: impl FnMut(Address) -> anyhow::Result<Account>,
    mut read_slot: impl FnMut(Address, SlotIndex) -> anyhow::Result<SlotValue>,
) -> anyhow::Result<()> {
    for address in actual.accounts.keys().copied().collect::<BTreeSet<_>>() {
        let changes = &actual.accounts[&address];
        if changes.nonce.is_changed() && changes.balance.is_changed() && changes.bytecode.is_changed() {
            continue;
        }
        let original = read_account(address)?;
        if let CompleteValue::Original(value) = &changes.nonce {
            ensure!(
                *value == original.nonce,
                "original nonce mismatch at {address}: actual={value}, expected={}",
                original.nonce
            );
        }
        if let CompleteValue::Original(value) = &changes.balance {
            ensure!(
                *value == original.balance,
                "original balance mismatch at {address}: actual={value}, expected={}",
                original.balance
            );
        }
        if let CompleteValue::Original(value) = &changes.bytecode {
            let actual_code = value.as_ref().map(RevmBytecode::original_byte_slice).unwrap_or_default();
            let original_code = original.bytecode.as_ref().map(RevmBytecode::original_byte_slice).unwrap_or_default();
            ensure!(actual_code == original_code, "original bytecode mismatch at {address}");
        }
    }
    for (address, index) in actual.slots.keys().copied().collect::<BTreeSet<_>>() {
        if let CompleteValue::Original(value) = &actual.slots[&(address, index)] {
            let original = read_slot(address, index)?;
            ensure!(
                *value == original,
                "original slot mismatch at {address}, index {index}: actual={value}, expected={original}"
            );
        }
    }
    Ok(())
}

/// Compares committed outcomes, resolving original fields and omitted entries from permanent prestate.
pub(super) fn compare_state_changes(
    actual: &State<Final>,
    expected: &State<Final>,
    mut read_account: impl FnMut(Address) -> anyhow::Result<Account>,
    mut read_slot: impl FnMut(Address, SlotIndex) -> anyhow::Result<SlotValue>,
) -> anyhow::Result<()> {
    for address in actual.accounts.keys().chain(expected.accounts.keys()).copied().collect::<BTreeSet<_>>() {
        let original = read_account(address)?;
        let resolve = |changes: Option<&AccountChanges<Final>>| Account {
            address,
            nonce: changes.and_then(|c| c.nonce.changed_ref()).copied().unwrap_or(original.nonce),
            balance: changes.and_then(|c| c.balance.changed_ref()).copied().unwrap_or(original.balance),
            bytecode: changes
                .and_then(|c| c.bytecode.changed_ref())
                .cloned()
                .unwrap_or_else(|| original.bytecode.clone()),
        };
        let actual = resolve(actual.accounts.get(&address));
        let expected = resolve(expected.accounts.get(&address));
        let (actual_nonce, expected_nonce) = (actual.nonce, expected.nonce);
        ensure!(
            actual_nonce == expected_nonce,
            "nonce mismatch at {address}: actual={actual_nonce}, expected={expected_nonce}"
        );
        let (actual_balance, expected_balance) = (actual.balance, expected.balance);
        ensure!(
            actual_balance == expected_balance,
            "balance mismatch at {address}: actual={actual_balance}, expected={expected_balance}"
        );
        let actual_code = actual.bytecode.as_ref().map(RevmBytecode::original_byte_slice).unwrap_or_default();
        let expected_code = expected.bytecode.as_ref().map(RevmBytecode::original_byte_slice).unwrap_or_default();
        ensure!(actual_code == expected_code, "bytecode mismatch at {address}");
    }
    for (address, index) in actual.slots.keys().chain(expected.slots.keys()).copied().collect::<BTreeSet<_>>() {
        let actual = actual.slots.get(&(address, index)).copied();
        let expected = expected.slots.get(&(address, index)).copied();
        let original = if actual.is_none() || expected.is_none() {
            read_slot(address, index)?
        } else {
            SlotValue::default()
        };
        let actual = actual.unwrap_or(original);
        let expected = expected.unwrap_or(original);
        ensure!(
            actual == expected,
            "slot mismatch at {address}, index {index}: actual={actual}, expected={expected}"
        );
    }
    Ok(())
}
