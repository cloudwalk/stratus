use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::anyhow;
use tracing::Span;

use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::evm::Evm;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutorError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecutionOutcome;

pub struct EvmTask<T: Task + Send> {
    pub span: Span,
    task: T,
}

#[derive(derive_new::new)]
pub struct ExecutionTask<Input: EvmInput> {
    pub input: Input,
    pub response_tx: oneshot::Sender<Result<(TransactionExecutionOutcome, EvmExecutionMetrics), StratusError>>,
}

#[derive(derive_new::new)]
pub struct InspectionTask {
    pub input: InspectorInput,
    pub response_tx: oneshot::Sender<Result<GethTrace, StratusError>>,
}

#[derive(Debug, Clone, strum::Display)]
pub enum EvmRoute {
    #[strum(to_string = "transaction")]
    Transaction(TransactionExecutionInput),

    #[strum(to_string = "call_present")]
    CallPresent(CallExecutionInput),

    #[strum(to_string = "call_past")]
    CallPast(CallExecutionInput),
}

impl<T: Task + Send> From<T> for EvmTask<T> {
    fn from(task: T) -> Self {
        Self { span: Span::current(), task }
    }
}

impl<T: Task + Send> EvmTask<T> {
    pub fn execute(self, evm: &mut Evm<T::Input>) -> anyhow::Result<(), StratusError> {
        let _enter = self.span.enter();
        catch_unwind(AssertUnwindSafe(|| self.task.execute(evm))).map_err(|err| ExecutorError::Panic { err: anyhow!("{err:?}") }.into())
    }
}

pub trait Task {
    type Input: EvmInput;

    fn execute(self, evm: &mut Evm<Self::Input>);
}

impl<Input: EvmInput> Task for ExecutionTask<Input> {
    type Input = Input;

    fn execute(self, evm: &mut Evm<Self::Input>) {
        if let Err(e) = self.response_tx.send(evm.execute(self.input)) {
            tracing::error!(reason = ?e, "failed to send evm task execution result");
        }
    }
}

impl Task for InspectionTask {
    type Input = TransactionExecutionInput;
    fn execute(self, evm: &mut Evm<TransactionExecutionInput>) {
        if let Err(e) = self.response_tx.send(evm.inspect(self.input)) {
            tracing::error!(reason = ?e, "failed to send evm task execution result");
        }
    }
}
