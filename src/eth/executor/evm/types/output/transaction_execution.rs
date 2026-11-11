use derive_more::Deref;
use derive_more::DerefMut;
use display_json::DebugAsJson;
use itertools::Itertools;
use revm::context::result::ExecutionResult as RevmExecutionResult;
use revm_state::EvmState;

use crate::eth::executor::ExecutionResult;
use crate::eth::executor::State;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::types::state::Complete;
use crate::eth::types::Address;
use crate::eth::types::Bytes;
use crate::eth::types::Gas;
use crate::eth::types::Log;
use crate::eth::types::StratusError;
use crate::ext::not;

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
