use std::collections::BTreeMap;

use alloy_primitives::B256;
use anyhow::Ok;
use anyhow::anyhow;
use display_json::DebugAsJson;
use hex_literal::hex;
use revm::primitives::alloy_primitives;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Log;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::log_and_err;

pub type ExecutionChanges = BTreeMap<Address, ExecutionAccountChanges>;

/// Output of a transaction executed in the EVM.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct EvmExecution {
    /// Assumed block timestamp during the execution.
    pub block_timestamp: UnixTime,

    /// Status of the execution.
    pub result: ExecutionResult,

    /// Output returned by the function execution (can be the function output or an exception).
    pub output: Bytes,

    /// Logs emitted by the function execution.
    pub logs: Vec<Log>,

    /// Consumed gas.
    pub gas: Gas,

    /// Storage changes that happened during the transaction execution.
    pub changes: ExecutionChanges,

    /// The contract address if the executed transaction deploys a contract.
    pub deployed_contract_address: Option<Address>,
}

impl EvmExecution {
    /// Creates an execution from an external transaction that failed.
    pub fn from_failed_external_transaction(sender: Account, receipt: &ExternalReceipt, block_timestamp: UnixTime) -> anyhow::Result<Self> {
        if receipt.is_success() {
            return log_and_err!("cannot create failed execution for successful transaction");
        }
        if not(receipt.inner.logs().is_empty()) {
            return log_and_err!("failed receipt should not have produced logs");
        }

        // generate sender changes incrementing the nonce
        let mut sender_changes = ExecutionAccountChanges::from_original_values(sender); // NOTE: don't change from_original_values without updating .expect() below
        let sender_next_nonce = sender_changes
            .nonce
            .take_original_ref()
            .ok_or_else(|| anyhow!("original nonce value not found when it should have been populated by from_original_values"))?
            .next_nonce();
        sender_changes.nonce.set_modified(sender_next_nonce);

        // crete execution and apply costs
        let mut execution = Self {
            block_timestamp,
            result: ExecutionResult::new_reverted("reverted externally".into()), // assume it reverted
            output: Bytes::default(),                                            // we cannot really know without performing an eth_call to the external system
            logs: Vec::new(),
            gas: Gas::from(receipt.gas_used),
            changes: BTreeMap::from([(sender_changes.address, sender_changes)]),
            deployed_contract_address: None,
        };
        execution.apply_receipt(receipt)?;
        Ok(execution)
    }

    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        matches!(self.result, ExecutionResult::Success)
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

    /// Checks if current execution state matches the information present in the external receipt.
    pub fn compare_with_receipt(&self, receipt: &ExternalReceipt) -> anyhow::Result<()> {
        // compare execution status
        if self.is_success() != receipt.is_success() {
            return log_and_err!(format!(
                "transaction status mismatch | hash={} execution={:?} receipt={:?}",
                receipt.hash(),
                self.result,
                receipt.status()
            ));
        }

        let receipt_logs = receipt.inner.logs();

        // check if any log has the specific topic that should skip validation
        const SKIP_VALIDATION_TOPIC: [u8; 32] = hex!("8d995e7fbf7a5ef41cee9e6936368925d88e07af89306bb78a698551562e683c");
        for receipt_log in receipt_logs.iter() {
            if let Some(first_topic) = receipt_log.topics().first() {
                let skip_topic = B256::from(SKIP_VALIDATION_TOPIC);
                if *first_topic == skip_topic {
                    return Ok(());
                }
            }
        }

        // compare logs length
        if self.logs.len() != receipt_logs.len() {
            tracing::trace!(logs = ?self.logs, "execution logs");
            tracing::trace!(logs = ?receipt_logs, "receipt logs");
            return log_and_err!(format!(
                "logs length mismatch | hash={} execution={} receipt={}",
                receipt.hash(),
                self.logs.len(),
                receipt_logs.len()
            ));
        }

        // compare logs pairs
        for (log_index, (execution_log, receipt_log)) in self.logs.iter().zip(receipt_logs).enumerate() {
            // compare log topics length
            if execution_log.topics_non_empty().len() != receipt_log.topics().len() {
                return log_and_err!(format!(
                    "log topics length mismatch | hash={} log_index={} execution={} receipt={}",
                    receipt.hash(),
                    log_index,
                    execution_log.topics_non_empty().len(),
                    receipt_log.topics().len(),
                ));
            }

            // compare log topics content
            // skip this log if the first topic matches the specific hash
            if !execution_log.topics_non_empty().is_empty() {
                let first_topic = B256::from(execution_log.topics_non_empty()[0]);
                let skip_topic = B256::from(hex!("029d06ff78b8c68fca5225af637c626c0176c9fcaa163beec8e558d4c3ae65b6"));
                if first_topic == skip_topic {
                    continue;
                }
            }

            for (topic_index, (execution_log_topic, receipt_log_topic)) in execution_log.topics_non_empty().iter().zip(receipt_log.topics().iter()).enumerate()
            {
                if B256::from(*execution_log_topic) != *receipt_log_topic {
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
            if execution_log.data.as_ref() != receipt_log.data().data.as_ref() {
                return log_and_err!(format!(
                    "log data content mismatch | hash={} log_index={} execution={} receipt={:#x}",
                    receipt.hash(),
                    log_index,
                    execution_log.data,
                    receipt_log.data().data,
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
        // fix gas
        self.gas = Gas::from(receipt.gas_used);

        // fix logs
        self.fix_logs_gas_left(receipt);

        // fix sender balance
        let execution_cost = receipt.execution_cost();

        if execution_cost > Wei::ZERO {
            // find sender changes
            let sender_address: Address = receipt.0.from.into();
            let Some(sender_changes) = self.changes.get_mut(&sender_address) else {
                return log_and_err!("sender changes not present in execution when applying execution costs");
            };

            // subtract execution cost from sender balance
            let sender_balance = *sender_changes.balance.take_ref().ok_or(anyhow!("sender balance was None"))?;

            let sender_new_balance = if sender_balance > execution_cost {
                sender_balance - execution_cost
            } else {
                Wei::ZERO
            };
            sender_changes.balance.set_modified(sender_new_balance);
        }

        Ok(())
    }

    /// Apply `gasLeft` values from receipt to execution logs.
    ///
    /// External transactions are re-executed locally with a different amount of gas limit, so, rely
    /// on the given receipt to copy the `gasLeft` values found in Logs.
    ///
    /// This is necessary if the contract emits an event that puts `gasLeft` in a log, this function
    /// covers the following events that do the described:
    ///
    /// - `ERC20Trace` (topic0: `0x31738ac4a7c9a10ecbbfd3fed5037971ba81b8f6aa4f72a23f5364e9bc76d671`)
    /// - `BalanceTrackerTrace` (topic0: `0x63f1e32b72965e2be75e03024856287aff9e4cdbcec65869c51014fc2c1c95d9`)
    ///
    /// The overwriting should be done by copying the first 32 bytes from the receipt to log in `self`.
    fn fix_logs_gas_left(&mut self, receipt: &ExternalReceipt) {
        const ERC20_TRACE_EVENT_HASH: [u8; 32] = hex!("31738ac4a7c9a10ecbbfd3fed5037971ba81b8f6aa4f72a23f5364e9bc76d671");
        const BALANCE_TRACKER_TRACE_EVENT_HASH: [u8; 32] = hex!("63f1e32b72965e2be75e03024856287aff9e4cdbcec65869c51014fc2c1c95d9");

        const EVENT_HASHES: [&[u8]; 2] = [&ERC20_TRACE_EVENT_HASH, &BALANCE_TRACKER_TRACE_EVENT_HASH];

        let receipt_logs = receipt.inner.logs();

        for (execution_log, receipt_log) in self.logs.iter_mut().zip(receipt_logs) {
            let execution_log_matches = || execution_log.topic0.is_some_and(|topic| EVENT_HASHES.contains(&topic.as_ref()));
            let receipt_log_matches = || receipt_log.topics().first().is_some_and(|topic| EVENT_HASHES.contains(&topic.as_ref()));

            // only try overwriting if both logs refer to the target event
            let should_overwrite = execution_log_matches() && receipt_log_matches();
            if !should_overwrite {
                continue;
            }

            let (Some(destination), Some(source)) = (execution_log.data.get_mut(0..32), receipt_log.data().data.get(0..32)) else {
                continue;
            };
            destination.copy_from_slice(source);
        }
    }
}

#[cfg(test)]
mod tests {
    use fake::Fake;
    use fake::Faker;

    use super::*;
    use crate::eth::primitives::CodeHash;
    use crate::eth::primitives::Nonce;

    #[test]
    fn test_from_failed_external_transaction() {
        // Create a mock sender account
        let sender_address: Address = Faker.fake();
        let sender = Account {
            address: sender_address,
            nonce: Nonce::from(1u64),
            balance: Wei::from(1000u64),
            bytecode: None,
            code_hash: CodeHash::default(),
        };

        // Create a mock failed receipt
        let mut receipt: ExternalReceipt = Faker.fake();
        let mut inner_receipt = receipt.0.clone();

        // Clear logs for failed transaction
        if let alloy_consensus::ReceiptEnvelope::Legacy(ref mut r) = inner_receipt.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(false);
            r.receipt.logs.clear();
        } else {
            panic!("expected be legacy!")
        }

        // Update from address
        inner_receipt.from = sender_address.into();
        receipt.0 = inner_receipt;

        // Set timestamp
        let timestamp = UnixTime::now();

        // Test the method
        let execution = EvmExecution::from_failed_external_transaction(sender.clone(), &receipt, timestamp).unwrap();

        // Verify execution state
        assert_eq!(execution.block_timestamp, timestamp);
        assert!(execution.is_failure());
        assert_eq!(execution.output, Bytes::default());
        assert!(execution.logs.is_empty());
        assert_eq!(execution.gas, Gas::from(receipt.gas_used));

        // Verify sender changes
        let sender_changes = execution.changes.get(&sender_address).unwrap();
        assert_eq!(sender_changes.address, sender_address);

        // Nonce should be incremented
        let modified_nonce = sender_changes.nonce.take_modified_ref().unwrap();
        assert_eq!(*modified_nonce, Nonce::from(2u64));

        // Balance should be reduced by execution cost
        if receipt.execution_cost() > Wei::ZERO {
            let modified_balance = sender_changes.balance.take_modified_ref().unwrap();
            assert!(sender.balance >= *modified_balance);
        }
    }

    #[test]
    fn test_compare_with_receipt_success_status_mismatch() {
        // Create a mock execution (success)
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;

        // Create a mock receipt (failed)
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(false);
        } else {
            panic!("expected be legacy!")
        }

        // Verify comparison fails
        assert!(execution.compare_with_receipt(&receipt).is_err());
    }

    #[test]
    fn test_compare_with_receipt_logs_length_mismatch() {
        // Create a mock execution with logs
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![Faker.fake(), Faker.fake()]; // Two logs

        // Create a mock receipt with different number of logs
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(true);
            r.receipt.logs = vec![alloy_rpc_types_eth::Log::default()]; // Only one log
        } else {
            panic!("expected be legacy!")
        }

        // Verify comparison fails
        assert!(execution.compare_with_receipt(&receipt).is_err());
    }

    #[test]
    fn test_compare_with_receipt_log_topics_length_mismatch() {
        // Create a mock log with topics
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(Faker.fake());
        log1.topic1 = Some(Faker.fake());
        log1.topic2 = None;
        log1.topic3 = None;

        // Create a mock execution with that log
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create receipt log with different number of topics
        let mut receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let topics = vec![B256::default()];
        receipt_log.inner.data = alloy_primitives::LogData::new_unchecked(topics, alloy_primitives::Bytes::default());
        // Only one topic instead of two

        // Create a receipt with this log
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(true);
            r.receipt.logs = vec![receipt_log.clone()];
        } else {
            panic!("expected be legacy!")
        }

        // Verify comparison fails
        assert!(execution.compare_with_receipt(&receipt).is_err());
    }

    #[test]
    fn test_compare_with_receipt_topic_content_mismatch() {
        // Create a topic
        let topic_value = B256::default();
        let different_topic = B256::default();

        // Create a mock log with the topic
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(topic_value.into());

        // Create execution with that log
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create receipt log with different topic content
        let mut receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let topics = vec![different_topic];
        receipt_log.inner.data = alloy_primitives::LogData::new_unchecked(topics, alloy_primitives::Bytes::default());

        // Create receipt with this log
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(true);
            r.receipt.logs = vec![receipt_log.clone()];
        } else {
            panic!("expected be legacy!")
        }

        // Verify comparison fails
        assert!(execution.compare_with_receipt(&receipt).is_err());
    }

    #[test]
    fn test_compare_with_receipt_data_content_mismatch() {
        // Create a mock log with data
        let mut log1: Log = Faker.fake();
        log1.topic0 = Some(Faker.fake());
        log1.data = vec![1, 2, 3, 4].into();

        // Create execution with that log
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;
        execution.logs = vec![log1];

        // Create receipt log with different data
        let mut receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let topics = vec![B256::default()];
        receipt_log.inner.data = alloy_primitives::LogData::new_unchecked(topics, alloy_primitives::Bytes::default());
        receipt_log.inner.data = alloy_primitives::LogData::new(vec![B256::default()], alloy_primitives::Bytes::from(vec![5, 6, 7, 8])).unwrap();

        // Create receipt with this log
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(true);
            r.receipt.logs = vec![receipt_log.clone()];
        } else {
            panic!("expected be legacy!")
        }

        // Verify comparison fails
        assert!(execution.compare_with_receipt(&receipt).is_err());
    }

    #[test]
    fn test_fix_logs_gas_left() {
        // Set up test constants
        const ERC20_TRACE_HASH: [u8; 32] = hex!("31738ac4a7c9a10ecbbfd3fed5037971ba81b8f6aa4f72a23f5364e9bc76d671");
        const BALANCE_TRACKER_TRACE_HASH: [u8; 32] = hex!("63f1e32b72965e2be75e03024856287aff9e4cdbcec65869c51014fc2c1c95d9");

        // Create a mock execution with logs that have gasLeft value we want to override
        let mut execution: EvmExecution = Faker.fake();
        execution.result = ExecutionResult::Success;

        // Create an ERC20 Trace log with mock gasLeft value
        let mut erc20_log: Log = Faker.fake();
        erc20_log.topic0 = Some(ERC20_TRACE_HASH.into());
        let execution_gas_left = vec![0u8; 32]; // Initial value all zeros
        let mut log_data = Vec::with_capacity(execution_gas_left.len() + 32);
        log_data.extend_from_slice(&execution_gas_left);
        log_data.extend_from_slice(&[99u8; 32]); // Add some additional data
        erc20_log.data = log_data.into();

        // Create a Balance Tracker Trace log
        let mut balance_log: Log = Faker.fake();
        balance_log.topic0 = Some(BALANCE_TRACKER_TRACE_HASH.into());
        let balance_gas_left = vec![0u8; 32]; // Initial value all zeros
        balance_log.data = balance_gas_left.into();

        // Create a regular log (not one we're targeting)
        let regular_log: Log = Faker.fake();

        execution.logs = vec![erc20_log, balance_log, regular_log.clone()];

        // Create receipt logs with different gasLeft values
        let receipt_erc20_gas_left = vec![42u8; 32]; // Different value for comparison
        let mut erc20_receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let erc20_topics = vec![B256::from_slice(&ERC20_TRACE_HASH)];

        let mut erc20_receipt_data = Vec::with_capacity(receipt_erc20_gas_left.len() + 32);
        erc20_receipt_data.extend_from_slice(&receipt_erc20_gas_left);
        erc20_receipt_data.extend_from_slice(&[99u8; 32]); // Match additional data

        erc20_receipt_log.inner.data = alloy_primitives::LogData::new_unchecked(erc20_topics, alloy_primitives::Bytes::from(erc20_receipt_data));

        // Balance tracker receipt log
        let receipt_balance_gas_left = vec![24u8; 32];
        let mut balance_receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let balance_topics = vec![B256::from_slice(&BALANCE_TRACKER_TRACE_HASH)];
        balance_receipt_log.inner.data =
            alloy_primitives::LogData::new_unchecked(balance_topics, alloy_primitives::Bytes::from(receipt_balance_gas_left.clone()));

        // Regular log for receipt
        let mut regular_receipt_log = alloy_rpc_types_eth::Log::<alloy_primitives::LogData>::default();
        let regular_topics = Vec::new();
        regular_receipt_log.inner.data = alloy_primitives::LogData::new_unchecked(regular_topics, alloy_primitives::Bytes::default());

        // Create receipt with these logs
        let mut receipt: ExternalReceipt = Faker.fake();
        if let alloy_consensus::ReceiptEnvelope::Legacy(r) = &mut receipt.0.inner {
            r.receipt.status = alloy_consensus::Eip658Value::Eip658(true);
            r.receipt.logs = vec![erc20_receipt_log.clone(), balance_receipt_log.clone(), regular_receipt_log.clone()];
        } else {
            panic!("expected be legacy!")
        }

        // Apply the fix
        execution.fix_logs_gas_left(&receipt);

        // Verify the first 32 bytes of ERC20 log data was overwritten
        let updated_erc20_data = execution.logs[0].data.as_ref();
        assert_eq!(&updated_erc20_data[0..32], &receipt_erc20_gas_left[..]);
        // Rest of the data should remain unchanged
        assert_eq!(&updated_erc20_data[32..], &[99u8; 32]);

        // Verify the first 32 bytes of Balance Tracker log data was overwritten
        assert_eq!(execution.logs[1].data.as_ref(), &receipt_balance_gas_left[..]);

        // Verify regular log data wasn't modified
        assert_eq!(execution.logs[2].data, regular_log.data);
    }

    #[test]
    fn test_apply_receipt() {
        // Create a mock sender account with balance
        let sender_address: Address = Faker.fake();
        let sender = Account {
            address: sender_address,
            nonce: Nonce::from(1u64),
            balance: Wei::from(1000u64),
            bytecode: None,
            code_hash: CodeHash::default(),
        };

        // Create a mock execution
        let mut execution: EvmExecution = Faker.fake();

        // Set up execution with sender account
        let sender_changes = ExecutionAccountChanges::from_original_values(sender);
        execution.changes = BTreeMap::from([(sender_address, sender_changes)]);
        execution.gas = Gas::from(100u64);

        // Create a receipt with higher gas used and execution cost
        let mut receipt: ExternalReceipt = Faker.fake();
        receipt.0.from = sender_address.into();
        receipt.0.gas_used = 100u64; // Higher gas

        // Make sure transaction has a cost
        let gas_price = Wei::from(1u64);
        receipt.0.effective_gas_price = gas_price.try_into().expect("wei was created with u64 which fits u128 qed.");

        // Apply receipt
        execution.apply_receipt(&receipt).unwrap();

        // Verify sender balance was reduced by execution cost
        let sender_changes = execution.changes.get(&sender_address).unwrap();
        let modified_balance = sender_changes.balance.take_modified_ref().unwrap();
        assert_eq!(*modified_balance, Wei::from(900u64)); // 1000 - 100
    }
}
