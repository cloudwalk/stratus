#[cfg(test)]
use alloy_primitives::B256;
use alloy_primitives::U256;
use derive_more::Deref;
use derive_more::DerefMut;
use display_json::DebugAsJson;
use hex_literal::hex;
use itertools::Itertools;
use revm::context::result::ExecutionResult as RevmExecutionResult;
use revm_state::EvmState;

use crate::eth::executor::ExecutionResult;
use crate::eth::executor::State;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::types::state::AccountChanges;
use crate::eth::executor::types::state::Complete;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Bytes;
use crate::eth::types::Gas;
use crate::eth::types::Hash;
use crate::eth::types::Log;
use crate::eth::types::StratusError;
use crate::eth::types::Wei;
use crate::ext::not;
use crate::log_and_err;

/// `ERC20Trace` event hash, whose first 32 data bytes carry the transaction `gasLeft` at emit time.
const ERC20_TRACE_EVENT_HASH: [u8; 32] = hex!("31738ac4a7c9a10ecbbfd3fed5037971ba81b8f6aa4f72a23f5364e9bc76d671");

/// `BalanceTrackerTrace` event hash, whose first 32 data bytes carry the transaction `gasLeft` at emit time.
const BALANCE_TRACKER_TRACE_EVENT_HASH: [u8; 32] = hex!("63f1e32b72965e2be75e03024856287aff9e4cdbcec65869c51014fc2c1c95d9");

/// Event hashes whose first 32 data bytes carry the transaction `gasLeft` at emit time.
const GAS_LEFT_EVENT_HASHES: [&[u8]; 2] = [&ERC20_TRACE_EVENT_HASH, &BALANCE_TRACKER_TRACE_EVENT_HASH];

/// Output of a transaction executed in the EVM.
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, Default, Deref, DerefMut)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecutionOutput {
    /// Status of the execution.
    #[deref]
    #[deref_mut]
    pub outcome: TransactionExecutionResult,

    /// Storage changes that happened during the transaction execution.
    pub state: State<Complete>,
}

/// Output of a transaction executed in the EVM.
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecutionResult {
    /// Status of the execution.
    pub result: ExecutionResult,

    /// Output returned by the function execution (can be the function output or an exception).
    pub output: Bytes,

    /// Logs emitted by the function execution.
    pub logs: Vec<Log>,

    /// Consumed gas.
    pub gas_used: Gas,

    /// The contract address if the executed transaction deploys a contract.
    pub deployed_contract_address: Option<Address>,
}

impl TransactionExecutionOutput {
    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
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

        None
    }

    /// Creates an execution from an imported stratus transaction that failed.
    pub fn from_failed_imported_transaction(sender: Account, gas_price: Wei, stored: &TransactionExecutionResult) -> anyhow::Result<Self> {
        if stored.result.is_success() {
            return log_and_err!("cannot create failed execution for successful transaction");
        }
        if not(stored.logs.is_empty()) {
            return log_and_err!("failed transaction should not have produced logs");
        }

        // generate sender changes incrementing the nonce
        let address = sender.address;
        let mut sender_changes = AccountChanges::default();
        sender_changes.apply_original(sender);
        let sender_next_nonce = sender_changes.nonce.next_nonce();

        sender_changes.nonce.apply(sender_next_nonce);
        let mut changes = State::default();
        changes.accounts.insert(address, sender_changes);

        // crete execution and apply costs
        let mut execution = Self {
            outcome: TransactionExecutionResult {
                result: ExecutionResult::new_reverted("reverted externally".into()), // assume it reverted
                output: Bytes::default(), // we cannot really know without performing an eth_call to the external system
                logs: Vec::new(),
                gas_used: stored.gas_used,
                deployed_contract_address: None,
            },
            state: changes,
        };
        execution.apply_imported(stored, gas_price, address)?;
        Ok(execution)
    }

    /// Checks if current execution state matches the stored execution of an imported transaction.
    pub fn compare_with_imported(&self, tx_hash: Hash, stored: &TransactionExecutionResult) -> anyhow::Result<()> {
        // compare execution status
        if self.is_success() != stored.result.is_success() {
            return log_and_err!(format!(
                "transaction status mismatch | hash={} execution={:?} imported={:?}",
                tx_hash, self.result, stored.result
            ));
        }

        // compare logs length
        if self.logs.len() != stored.logs.len() {
            tracing::trace!(logs = ?self.logs, "execution logs");
            tracing::trace!(logs = ?stored.logs, "imported logs");
            return log_and_err!(format!(
                "logs length mismatch | hash={} execution={} imported={}",
                tx_hash,
                self.logs.len(),
                stored.logs.len()
            ));
        }

        // compare logs pairs
        for (log_index, (execution_log, imported_log)) in self.logs.iter().zip(&stored.logs).enumerate() {
            // compare log topics length
            if execution_log.topics_non_empty().len() != imported_log.topics_non_empty().len() {
                return log_and_err!(format!(
                    "log topics length mismatch | hash={} log_index={} execution={} imported={}",
                    tx_hash,
                    log_index,
                    execution_log.topics_non_empty().len(),
                    imported_log.topics_non_empty().len(),
                ));
            }

            // compare log topics content
            for (topic_index, (execution_log_topic, imported_log_topic)) in
                execution_log.topics_non_empty().iter().zip(imported_log.topics_non_empty().iter()).enumerate()
            {
                if execution_log_topic != imported_log_topic {
                    return log_and_err!(format!(
                        "log topic content mismatch | hash={} log_index={} topic_index={} execution={:#x} imported={:#x}",
                        tx_hash, log_index, topic_index, execution_log_topic.0, imported_log_topic.0,
                    ));
                }
            }

            // compare log data content
            if execution_log.data.as_ref() != imported_log.data.as_ref() {
                return log_and_err!(format!(
                    "log data content mismatch | hash={} log_index={} execution={} imported={}",
                    tx_hash, log_index, execution_log.data, imported_log.data,
                ));
            }
        }
        Ok(())
    }

    /// Imported transactions are re-executed locally with max gas and zero gas price.
    ///
    /// This causes some attributes to be different from the stored execution.
    ///
    /// This method updates the attributes that can diverge based on the stored execution of the imported transaction.
    pub fn apply_imported(&mut self, stored: &TransactionExecutionResult, gas_price: Wei, sender: Address) -> anyhow::Result<()> {
        // fix gas
        self.gas_used = stored.gas_used;

        // fix logs
        self.fix_logs_gas_left_from_stored(&stored.logs);

        // fix sender balance
        let execution_cost = Wei(gas_price.0 * U256::from(stored.gas_used.as_u64()));

        if execution_cost > Wei::ZERO {
            // find sender changes
            let Some(sender_changes) = self.state.accounts.get_mut(&sender) else {
                return log_and_err!("sender changes not present in execution when applying execution costs");
            };

            // subtract execution cost from sender balance
            let sender_balance = *sender_changes.balance.value();

            let sender_new_balance = if sender_balance > execution_cost {
                sender_balance - execution_cost
            } else {
                Wei::ZERO
            };
            sender_changes.balance.apply(sender_new_balance);
        }

        Ok(())
    }

    /// Apply `gasLeft` values from the stored logs to the execution logs.
    ///
    /// Imported transactions are re-executed locally with a different amount of gas limit, so rely
    /// on the stored logs to copy the `gasLeft` values.
    fn fix_logs_gas_left_from_stored(&mut self, stored_logs: &[Log]) {
        for (execution_log, stored_log) in self.logs.iter_mut().zip(stored_logs) {
            let execution_log_matches = || execution_log.topic0.is_some_and(|topic| GAS_LEFT_EVENT_HASHES.contains(&topic.0.as_ref()));
            let stored_log_matches = || stored_log.topic0.is_some_and(|topic| GAS_LEFT_EVENT_HASHES.contains(&topic.0.as_ref()));

            // only try overwriting if both logs refer to the target event
            let should_overwrite = execution_log_matches() && stored_log_matches();
            if !should_overwrite {
                continue;
            }

            let Some(source) = stored_log.data.as_ref().get(0..32) else {
                continue;
            };
            let mut data = execution_log.data.0.to_vec();
            let Some(destination) = data.get_mut(0..32) else {
                continue;
            };
            destination.copy_from_slice(source);
            execution_log.data = Bytes::from(data);
        }
    }

    fn parse_revm_result(result: RevmExecutionResult) -> (ExecutionResult, Bytes, Vec<Log>, Gas) {
        match result {
            RevmExecutionResult::Success { output, gas, logs, .. } => {
                let result = ExecutionResult::Success;
                let output = Bytes::from(output);
                let logs = logs.into_iter().map_into().collect();
                let gas = Gas::from(gas);
                (result, output, logs, gas)
            }
            RevmExecutionResult::Revert { output, gas, logs } => {
                let output = Bytes::from(output);
                let result = ExecutionResult::Reverted { reason: (&output).into() };
                let gas = Gas::from(gas);
                let logs = logs.into_iter().map_into().collect();
                (result, output, logs, gas)
            }
            RevmExecutionResult::Halt { reason, gas, logs } => {
                let result = ExecutionResult::new_halted(format!("{reason:?}"));
                let output = Bytes::default();
                let gas = Gas::from(gas);
                let logs = logs.into_iter().map_into().collect();
                (result, output, logs, gas)
            }
        }
    }

    fn parse_revm_state(revm_state: EvmState) -> Result<(State<Complete>, Option<Address>), StratusError> {
        let mut deployed_contract_address = None;
        let mut execution_changes = State::default();
        // might be improved by only keeping slots read from perm and modified slots
        // and discard stots found in temp and cache
        for (revm_address, mut revm_account) in revm_state {
            let address: Address = revm_address.into();

            if address.is_ignored() {
                continue;
            }

            tracing::debug!(
                %address,
                status = ?revm_account.status,
                balance = %revm_account.info.balance,
                nonce = %revm_account.info.nonce,
                slots = %revm_account.storage.len(),
                "evm account"
            );

            if revm_account.is_created() && revm_account.info.code.is_some() {
                deployed_contract_address = Some(address);
            }

            let storage = std::mem::take(&mut revm_account.storage);
            let account_slots = storage.into_iter().map(|(index, value)| (index.into(), value.into())).collect();

            execution_changes.insert_slots(address, account_slots);
            execution_changes.insert_account(address, revm_account.into());
        }
        Ok((execution_changes, deployed_contract_address))
    }
}

impl TryFrom<RevmResultAndState> for TransactionExecutionOutput {
    type Error = StratusError;

    fn try_from(value: RevmResultAndState) -> Result<Self, Self::Error> {
        let (result, tx_output, logs, gas) = Self::parse_revm_result(value.result);
        let (changes, deployed_contract_address) = Self::parse_revm_state(value.state)?;
        tracing::debug!(?result, %gas, tx_output_len = %tx_output.len(), %tx_output, "evm executed");

        Ok(TransactionExecutionOutput {
            outcome: TransactionExecutionResult {
                result,
                output: tx_output,
                logs,
                gas_used: gas,
                deployed_contract_address,
            },
            state: changes,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fake::Fake;
    use fake::Faker;

    use super::*;
    use crate::eth::types::Nonce;

    #[test]
    fn test_from_failed_imported_transaction() {
        // Create a mock sender account
        let sender_address: Address = Faker.fake();
        let sender = Account {
            address: sender_address,
            nonce: Nonce::from(1u64),
            balance: Wei::from(1000u64),
            bytecode: None,
        };

        // Create a stored failed execution
        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::new_reverted("reverted externally".into());
        stored.logs = Vec::new();
        stored.gas_used = Gas::from(100u64);

        // Test the method
        let gas_price = Wei::from(1u64);
        let execution = TransactionExecutionOutput::from_failed_imported_transaction(sender.clone(), gas_price, &stored).unwrap();

        // Verify execution state
        assert!(execution.is_failure());
        assert_eq!(execution.output, Bytes::default());
        assert!(execution.logs.is_empty());
        assert_eq!(execution.gas_used, Gas::from(100u64));

        // Verify sender changes
        let sender_changes = execution.state.accounts.get(&sender_address).unwrap();

        // Nonce should be incremented
        let modified_nonce = *sender_changes.nonce.value();
        assert_eq!(modified_nonce, Nonce::from(2u64));

        // Balance should be reduced by execution cost (gas price * gas used)
        let modified_balance = *sender_changes.balance.value();
        assert_eq!(modified_balance, Wei::from(900u64)); // 1000 - 1 * 100

        // Guard: successful stored executions are rejected
        let mut success_stored = stored.clone();
        success_stored.result = ExecutionResult::Success;
        assert!(TransactionExecutionOutput::from_failed_imported_transaction(sender, gas_price, &success_stored).is_err());
    }

    #[test]
    fn test_compare_with_imported_matching_execution_ok() {
        // Create a mock execution and stored execution with identical content
        let mut log: Log = Faker.fake();
        log.topic0 = Some(B256::from([1u8; 32]).into());
        log.topic1 = None;
        log.topic2 = None;
        log.topic3 = None;
        log.data = vec![1, 2, 3, 4].into();

        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log.clone()];

        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::Success;
        stored.logs = vec![log];

        let tx_hash: Hash = Faker.fake();
        assert!(execution.compare_with_imported(tx_hash, &stored).is_ok());
    }

    #[test]
    fn test_compare_with_imported_status_mismatch() {
        // Create a mock execution (success)
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;

        // Create a stored execution (failed)
        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::new_reverted("reverted externally".into());

        // Verify comparison fails
        let tx_hash: Hash = Faker.fake();
        assert!(execution.compare_with_imported(tx_hash, &stored).is_err());
    }

    #[test]
    fn test_compare_with_imported_logs_length_mismatch() {
        // Create a mock execution with logs
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![Faker.fake(), Faker.fake()]; // Two logs

        // Create a stored execution with only one log
        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::Success;
        stored.logs = vec![Faker.fake()]; // Only one log

        // Verify comparison fails
        let tx_hash: Hash = Faker.fake();
        assert!(execution.compare_with_imported(tx_hash, &stored).is_err());
    }

    #[test]
    fn test_compare_with_imported_log_topics_length_mismatch() {
        // Create a mock log with two topics
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(Faker.fake());
        log1.topic1 = Some(Faker.fake());
        log1.topic2 = None;
        log1.topic3 = None;

        // Create a mock execution with that log
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create a stored log with only one topic
        let mut stored_log: Log = Faker.fake();
        stored_log.topic0 = Some(Faker.fake());
        stored_log.topic1 = None;
        stored_log.topic2 = None;
        stored_log.topic3 = None;

        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::Success;
        stored.logs = vec![stored_log];

        // Verify comparison fails
        let tx_hash: Hash = Faker.fake();
        assert!(execution.compare_with_imported(tx_hash, &stored).is_err());
    }

    #[test]
    fn test_compare_with_imported_topic_content_mismatch() {
        // Create two genuinely different topics
        let topic_value = B256::from([1u8; 32]);
        let different_topic = B256::from([2u8; 32]);

        // Create a mock log with only topic0 set
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(topic_value.into());
        log1.topic1 = None;
        log1.topic2 = None;
        log1.topic3 = None;
        log1.data = vec![].into();

        // Create execution with that log
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create a stored log with the same number of topics but different content
        let mut stored_log: Log = Faker.fake();
        stored_log.topic0 = Some(different_topic.into());
        stored_log.topic1 = None;
        stored_log.topic2 = None;
        stored_log.topic3 = None;
        stored_log.data = vec![].into();

        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::Success;
        stored.logs = vec![stored_log];

        // Verify comparison fails due to topic content mismatch
        let tx_hash: Hash = Faker.fake();
        let err = execution.compare_with_imported(tx_hash, &stored).unwrap_err();
        assert!(err.to_string().contains("log topic content mismatch"));
    }

    #[test]
    fn test_compare_with_imported_data_content_mismatch() {
        // Create a mock log with data
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(Faker.fake());
        log1.data = vec![1, 2, 3, 4].into();

        // Create execution with that log
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create a stored log with different data
        let mut stored_log: Log = Faker.fake();
        stored_log.topic0 = Some(Faker.fake());
        stored_log.data = vec![5, 6, 7, 8].into();

        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.result = ExecutionResult::Success;
        stored.logs = vec![stored_log];

        // Verify comparison fails
        let tx_hash: Hash = Faker.fake();
        assert!(execution.compare_with_imported(tx_hash, &stored).is_err());
    }

    #[test]
    fn test_apply_imported() {
        // Create a mock sender account with balance
        let sender_address: Address = Faker.fake();
        let sender = Account {
            address: sender_address,
            nonce: Nonce::from(1u64),
            balance: Wei::from(1000u64),
            bytecode: None,
        };

        // Create a mock execution with the sender account in its state
        let mut execution: TransactionExecutionOutput = Faker.fake();
        let mut sender_changes = AccountChanges::default();
        sender_changes.apply_original(sender);
        let mut accounts = HashMap::with_hasher(foldhash::fast::RandomState::default());
        accounts.insert(sender_address, sender_changes);
        execution.state = State {
            accounts,
            ..Default::default()
        };
        execution.gas_used = Gas::from(100u64);

        // Create a stored execution with different gas
        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.gas_used = Gas::from(200u64);
        stored.logs = Vec::new();

        // Apply the stored execution
        let gas_price = Wei::from(3u64);
        execution.apply_imported(&stored, gas_price, sender_address).unwrap();

        // Gas used should match the stored execution
        assert_eq!(execution.gas_used, Gas::from(200u64));

        // Sender balance should be reduced by gas price * stored gas (1000 - 3 * 200 = 400)
        let sender_changes = execution.state.accounts.get(&sender_address).unwrap();
        let modified_balance = *sender_changes.balance.value();
        assert_eq!(modified_balance, Wei::from(400u64));
    }

    #[test]
    fn test_apply_imported_balance_floors_at_zero() {
        // Create a mock sender account with a balance smaller than the execution cost
        let sender_address: Address = Faker.fake();
        let sender = Account {
            address: sender_address,
            nonce: Nonce::from(1u64),
            balance: Wei::from(100u64),
            bytecode: None,
        };

        // Create a mock execution with the sender account in its state
        let mut execution: TransactionExecutionOutput = Faker.fake();
        let mut sender_changes = AccountChanges::default();
        sender_changes.apply_original(sender);
        let mut accounts = HashMap::with_hasher(foldhash::fast::RandomState::default());
        accounts.insert(sender_address, sender_changes);
        execution.state = State {
            accounts,
            ..Default::default()
        };

        // Stored execution with a cost far above the balance (3 * 200 = 600 > 100)
        let mut stored: TransactionExecutionResult = Faker.fake();
        stored.gas_used = Gas::from(200u64);
        stored.logs = Vec::new();

        let gas_price = Wei::from(3u64);
        execution.apply_imported(&stored, gas_price, sender_address).unwrap();

        // Sender balance should floor at zero instead of underflowing
        let sender_changes = execution.state.accounts.get(&sender_address).unwrap();
        let modified_balance = *sender_changes.balance.value();
        assert_eq!(modified_balance, Wei::ZERO);
    }

    #[test]
    fn test_fix_logs_gas_left_from_stored() {
        // Set up test constants
        const ERC20_TRACE_HASH: [u8; 32] = hex!("31738ac4a7c9a10ecbbfd3fed5037971ba81b8f6aa4f72a23f5364e9bc76d671");
        const BALANCE_TRACKER_TRACE_HASH: [u8; 32] = hex!("63f1e32b72965e2be75e03024856287aff9e4cdbcec65869c51014fc2c1c95d9");

        // Create an execution with logs that have gasLeft values we want to override
        let mut execution: TransactionExecutionOutput = Faker.fake();
        execution.result = ExecutionResult::Success;

        // Create an ERC20 Trace log with mock gasLeft value
        let mut erc20_log: Log = Faker.fake();
        erc20_log.topic0 = Some(ERC20_TRACE_HASH.into());
        let execution_gas_left = vec![0u8; 32];
        let mut log_data = Vec::with_capacity(execution_gas_left.len() + 32);
        log_data.extend_from_slice(&execution_gas_left);
        log_data.extend_from_slice(&[99u8; 32]);
        erc20_log.data = log_data.into();

        // Create a Balance Tracker Trace log
        let mut balance_log: Log = Faker.fake();
        balance_log.topic0 = Some(BALANCE_TRACKER_TRACE_HASH.into());
        let balance_gas_left = vec![0u8; 32];
        balance_log.data = balance_gas_left.into();

        // Create a regular log (not one we're targeting)
        let regular_log: Log = Faker.fake();

        execution.logs = vec![erc20_log, balance_log, regular_log.clone()];

        // Create stored logs with different gasLeft values
        let receipt_erc20_gas_left = vec![42u8; 32];
        let mut stored_erc20_log: Log = Faker.fake();
        stored_erc20_log.topic0 = Some(ERC20_TRACE_HASH.into());
        let mut stored_erc20_data = Vec::with_capacity(receipt_erc20_gas_left.len() + 32);
        stored_erc20_data.extend_from_slice(&receipt_erc20_gas_left);
        stored_erc20_data.extend_from_slice(&[99u8; 32]);
        stored_erc20_log.data = stored_erc20_data.into();

        let receipt_balance_gas_left = vec![24u8; 32];
        let mut stored_balance_log: Log = Faker.fake();
        stored_balance_log.topic0 = Some(BALANCE_TRACKER_TRACE_HASH.into());
        stored_balance_log.data = receipt_balance_gas_left.clone().into();

        // Regular stored log (topic0 will not match the target hashes)
        let stored_regular_log: Log = Faker.fake();

        let stored = TransactionExecutionResult {
            result: ExecutionResult::Success,
            output: Bytes::default(),
            logs: vec![stored_erc20_log, stored_balance_log, stored_regular_log],
            gas_used: Gas::default(),
            deployed_contract_address: None,
        };

        // Apply the fix
        execution.fix_logs_gas_left_from_stored(&stored.logs);

        // Verify the first 32 bytes of ERC20 log data was overwritten
        let updated_erc20_data = execution.logs[0].data.as_ref();
        assert_eq!(&updated_erc20_data[0..32], &receipt_erc20_gas_left[..]);
        // Rest of the data should remain unchanged
        assert_eq!(&updated_erc20_data[32..], &[99u8; 32]);

        // Verify the first 32 bytes of Balance Tracker log data was overwritten
        assert_eq!(execution.logs[1].data.as_ref()[..32].to_vec(), receipt_balance_gas_left);

        // Verify regular log data was not modified
        assert_eq!(execution.logs[2].data, regular_log.data);
    }
}
