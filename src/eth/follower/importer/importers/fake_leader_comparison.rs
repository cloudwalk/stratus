use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::bail;
use anyhow::ensure;
use serde_json::Value;

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
use crate::eth::types::Block;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;

/// Reports the first differing header or transaction field while preserving block equality.
pub(super) fn compare_blocks(actual: &Block, expected: &Block) -> anyhow::Result<()> {
    if actual.header != expected.header {
        return report_difference("header", &actual.header, &expected.header);
    }
    ensure!(
        actual.transactions.len() == expected.transactions.len(),
        "transactions.length mismatch: actual={}, expected={}",
        actual.transactions.len(),
        expected.transactions.len()
    );
    for (index, (actual, expected)) in actual.transactions.iter().zip(&expected.transactions).enumerate() {
        if actual != expected {
            ensure!(
                actual.execution.input.gas_price == expected.execution.input.gas_price,
                "transactions[{index}].execution.input.gas_price mismatch: actual={}, expected={}",
                actual.execution.input.gas_price,
                expected.execution.input.gas_price
            );
            return report_difference(&format!("transactions[{index}]"), actual, expected);
        }
    }
    Ok(())
}

fn report_difference(path: &str, actual: &impl serde::Serialize, expected: &impl serde::Serialize) -> anyhow::Result<()> {
    let actual = serde_json::from_str(&serde_json::to_string(actual)?)?;
    let expected = serde_json::from_str(&serde_json::to_string(expected)?)?;
    if let Some(difference) = first_difference(path, &actual, &expected) {
        bail!(difference);
    }
    bail!("{path} differs in its internal representation")
}

fn first_difference(path: &str, actual: &Value, expected: &Value) -> Option<String> {
    if actual == expected {
        return None;
    }
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) =>
            for key in actual.keys().chain(expected.keys()).collect::<BTreeSet<_>>() {
                let field = format!("{path}.{key}");
                match (actual.get(key), expected.get(key)) {
                    (Some(actual), Some(expected)) =>
                        if let Some(difference) = first_difference(&field, actual, expected) {
                            return Some(difference);
                        },
                    (actual, expected) => {
                        return Some(format!(
                            "{field} presence mismatch: actual={}, expected={}",
                            actual.is_some(),
                            expected.is_some()
                        ));
                    }
                }
            },
        (Value::Array(actual), Value::Array(expected)) => {
            if actual.len() != expected.len() {
                return Some(format!("{path}.length mismatch: actual={}, expected={}", actual.len(), expected.len()));
            }
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                if let Some(difference) = first_difference(&format!("{path}[{index}]"), actual, expected) {
                    return Some(difference);
                }
            }
        }
        _ => return Some(format!("{path} mismatch: actual={}, expected={}", summarize(actual), summarize(expected))),
    }
    None
}

fn summarize(value: &Value) -> String {
    match value {
        Value::String(value) => format!("{:?} ({} bytes)", value.chars().take(96).collect::<String>(), value.len()),
        Value::Array(value) => format!("array ({} elements)", value.len()),
        Value::Object(value) => format!("object ({} fields)", value.len()),
        value => value.to_string(),
    }
}

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

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;
    use serde_json::json;

    use super::compare_blocks;
    use super::first_difference;
    use crate::eth::types::Block;
    use crate::eth::types::Gas;
    use crate::eth::types::Index;
    use crate::eth::types::TransactionMined;

    #[test]
    fn accepts_equal_blocks() {
        let block = Block::genesis();
        compare_blocks(&block, &block).unwrap();
    }

    #[test]
    fn reports_header_field() {
        let expected = Block::genesis();
        let mut actual = expected.clone();
        actual.header.gas_used = Gas::from(42u64);
        let error = compare_blocks(&actual, &expected).unwrap_err().to_string();
        assert!(error.contains("header.gas_used"));
        assert!(error.contains("0x2a"));
    }

    #[test]
    fn reports_transaction_count_and_mined_metadata() {
        let mut expected = Block::genesis();
        let mut transaction: TransactionMined = Faker.fake();
        transaction.execution.output.logs.clear();
        transaction.mined_data.first_log_index = Index::ZERO;
        expected.transactions.push(transaction);
        let mut actual = expected.clone();
        actual.transactions[0].mined_data.first_log_index = Index(5);
        let error = compare_blocks(&actual, &expected).unwrap_err().to_string();
        assert!(error.contains("transactions[0].mined_data.first_log_index"), "{error}");
        actual.transactions.clear();
        assert!(compare_blocks(&actual, &expected).unwrap_err().to_string().contains("transactions.length"));
    }

    #[test]
    fn reports_execution_result_difference() {
        let mut expected = Block::genesis();
        let mut transaction: TransactionMined = Faker.fake();
        transaction.execution.output.gas_used = Gas::from(1u64);
        expected.transactions.push(transaction);
        let mut actual = expected.clone();
        actual.transactions[0].execution.output.gas_used = Gas::from(2u64);
        let error = compare_blocks(&actual, &expected).unwrap_err().to_string();
        assert!(error.contains("transactions[0].execution.output.gas_used"), "{error}");
    }

    #[test]
    fn reports_large_gas_prices_exactly() {
        let mut expected = Block::genesis();
        let mut transaction: TransactionMined = Faker.fake();
        transaction.execution.input.gas_price = u128::MAX;
        expected.transactions.push(transaction);
        let mut actual = expected.clone();
        actual.transactions[0].execution.input.gas_price -= 1;
        let error = compare_blocks(&actual, &expected).unwrap_err().to_string();
        assert!(error.contains("transactions[0].execution.input.gas_price"));
        assert!(error.contains(&u128::MAX.to_string()));
        assert!(error.contains(&(u128::MAX - 1).to_string()));
    }

    #[test]
    fn bounds_nested_values_and_reports_array_positions() {
        let actual = json!({"logs": [{"data": "a".repeat(100_000)}]});
        let expected = json!({"logs": [{"data": "b".repeat(100_000)}]});
        let difference = first_difference("output", &actual, &expected).unwrap();
        assert!(difference.starts_with("output.logs[0].data mismatch"));
        assert!(difference.len() < 400);
        assert!(difference.contains("100000 bytes"));
    }
}
