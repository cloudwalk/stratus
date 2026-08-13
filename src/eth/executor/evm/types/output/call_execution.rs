use display_json::DebugAsJson;
use revm::context::result::ExecutionResult as RevmExecutionResult;

use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::types::Bytes;
use crate::eth::types::Gas;
use crate::eth::types::StratusError;

/// Output of a transaction executed in the EVM.
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct CallExecutionOutput {
    /// Output returned by the function execution (can be the function output or an exception).
    pub output: Bytes,

    /// Consumed gas.
    pub gas_used: Gas,

    /// If the tx finished successfully (no reverts or halts)
    pub success: bool,
}

impl CallExecutionOutput {
    fn parse_revm_result(result: RevmExecutionResult) -> (Bytes, Gas, bool) {
        match result {
            RevmExecutionResult::Success { output, gas, .. } => {
                let output = Bytes::from(output);
                let gas = Gas::from(gas);
                (output, gas, true)
            }
            RevmExecutionResult::Revert { output, gas, .. } => {
                let output = Bytes::from(output);
                let gas = Gas::from(gas);
                (output, gas, false)
            }
            RevmExecutionResult::Halt { gas, .. } => {
                let output = Bytes::default();
                let gas = Gas::from(gas);

                (output, gas, false)
            }
        }
    }
}

impl TryFrom<RevmResultAndState> for CallExecutionOutput {
    type Error = StratusError;

    fn try_from(value: RevmResultAndState) -> Result<Self, Self::Error> {
        let (output, gas_used, success) = Self::parse_revm_result(value.result);
        Ok(CallExecutionOutput { output, gas_used, success })
    }
}
