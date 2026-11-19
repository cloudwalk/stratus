use derive_more::Deref;
use derive_more::DerefMut;
use display_json::DebugAsJson;
use itertools::Itertools;
use revm::context::result::ExecutionResult as RevmExecutionResult;
use revm::context::result::Output as RevmOutput;
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

    /// Address of some contract created during the execution, if any.
    ///
    /// It is NOT the contract deployed by the transaction: when the transaction creates more than one contract
    /// (a constructor or a factory deploying others), it is an arbitrary one of them, and it is also set for calls (`to` set)
    /// that deploy through a factory. Treat it as a flag telling whether the transaction created contracts.
    ///
    /// The receipt `contractAddress` is derived from the transaction sender and nonce instead.
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

    fn parse_revm_result(result: RevmExecutionResult) -> (ExecutionResult, Bytes, Vec<Log>, Gas, Option<Address>) {
        match result {
            RevmExecutionResult::Success { output, gas, logs, .. } => {
                let result = ExecutionResult::Success;
                let (output, deployed_contract_address) = match output {
                    RevmOutput::Call(inner) => (Bytes::from(inner), None),
                    RevmOutput::Create(inner, address) => (Bytes::from(inner), address.map(Into::into)),
                };
                let logs = logs.into_iter().map_into().collect();
                let gas = Gas::from(gas);
                (result, output, logs, gas, deployed_contract_address)
            }
            RevmExecutionResult::Revert { output, gas, logs } => {
                let output = Bytes::from(output);
                let result = ExecutionResult::Reverted { reason: (&output).into() };
                let gas = Gas::from(gas);
                let logs = logs.into_iter().map_into().collect();
                (result, output, logs, gas, None)
            }
            RevmExecutionResult::Halt { reason, gas, logs } => {
                let result = ExecutionResult::new_halted(format!("{reason:?}"));
                let output = Bytes::default();
                let gas = Gas::from(gas);
                let logs = logs.into_iter().map_into().collect();
                (result, output, logs, gas, None)
            }
        }
    }

    fn parse_revm_state(revm_state: EvmState) -> Result<State<Complete>, StratusError> {
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
                slots = %revm_account.storage.len(),
                "evm account"
            );

            let storage = std::mem::take(&mut revm_account.storage);
            let account_slots = storage.into_iter().map(|(index, value)| (index.into(), value.into())).collect();

            execution_changes.insert_slots(address, account_slots);
            execution_changes.insert_account(address, revm_account.into());
        }
        Ok(execution_changes)
    }
}

impl TryFrom<RevmResultAndState> for TransactionExecutionOutput {
    type Error = StratusError;

    fn try_from(value: RevmResultAndState) -> Result<Self, Self::Error> {
        let (result, tx_output, logs, gas, deployed_contract_address) = Self::parse_revm_result(value.result);
        let changes = Self::parse_revm_state(value.state)?;
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
    use hex_literal::hex;
    use revm::context::result::ExecutionResult as RevmExecutionResult;
    use revm::context::result::Output as RevmOutput;
    use revm::context::result::ResultGas;
    use revm::context::result::SuccessReason;
    use revm_state::EvmState;

    use super::TransactionExecutionOutput;
    use crate::eth::executor::evm::RevmResultAndState;
    use crate::eth::types::Address;

    /// The deployed contract address must come from the create outcome reported by the EVM, not
    /// from scanning the post-execution state, which is not deterministic when more than one
    /// account is created during the transaction (e.g. UUPS proxies that also touch their
    /// implementation). Regression test for the leader/follower mismatch that killed followers
    /// importing blocks with such transactions.
    #[test]
    fn deployed_contract_address_comes_from_the_create_outcome() {
        let created = Address::new(hex!("cafac3dd18ac6c6e92c921884f9e4176737c052c"));

        let result = RevmExecutionResult::Success {
            reason: SuccessReason::Return,
            gas: ResultGas::new_with_state_gas(1, 0, 0, 0),
            logs: vec![],
            output: RevmOutput::Create(alloy_primitives::Bytes::default(), Some(created.into())),
        };
        let output = TransactionExecutionOutput::try_from(RevmResultAndState {
            result,
            state: EvmState::default(),
        })
        .expect("parse revm result");

        assert_eq!(output.outcome.deployed_contract_address, Some(created));
    }

    /// Plain calls never report a deployed contract address, even if the post-execution state
    /// contains accounts that were created by inner calls of the transaction.
    #[test]
    fn deployed_contract_address_is_none_for_calls() {
        let result = RevmExecutionResult::Success {
            reason: SuccessReason::Return,
            gas: ResultGas::new_with_state_gas(1, 0, 0, 0),
            logs: vec![],
            output: RevmOutput::Call(alloy_primitives::Bytes::default()),
        };
        let output = TransactionExecutionOutput::try_from(RevmResultAndState {
            result,
            state: EvmState::default(),
        })
        .expect("parse revm result");

        assert_eq!(output.outcome.deployed_contract_address, None);
    }
}
