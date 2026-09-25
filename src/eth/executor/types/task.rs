use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::anyhow;
use tracing::Span;

use crate::eth::executor::evm::Evm;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::EvmInput;
use crate::eth::executor::evm::types::EvmKind;
use crate::eth::executor::evm::types::ExecutionMetrics;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::types::error::ExecutorError;
use crate::eth::types::StratusError;
use crate::utils::Permit;

#[derive(derive_new::new)]
pub struct ExecutionTask<Input: EvmInput> {
    pub input: Input,
    pub response_tx: oneshot::Sender<Result<(RevmResultAndState, ExecutionMetrics), StratusError>>,
}

#[derive(derive_new::new)]
pub struct InspectionTask {
    pub input: InspectorInput,
    pub response_tx: oneshot::Sender<Result<GethTrace, StratusError>>,
}

#[derive(Debug, Clone, strum::Display)]
pub enum EvmRoute {
    #[strum(to_string = "call_present")]
    CallPresent(CallExecutionInput),

    #[strum(to_string = "call_past")]
    CallPast(CallExecutionInput),
}

/// A task for the unified EVM pool.
pub struct PoolTask {
    pub span: Span,
    kind: EvmKind,
    permit: Permit,
    task: PoolTaskKind,
}

enum PoolTaskKind {
    Call(ExecutionTask<CallExecutionInput>),
    Inspect(InspectionTask),
}

impl PoolTask {
    pub fn call(task: ExecutionTask<CallExecutionInput>, kind: EvmKind, permit: Permit) -> Self {
        debug_assert!(matches!(kind, EvmKind::CallPresent | EvmKind::CallPast));
        Self {
            span: Span::current(),
            kind,
            permit,
            task: PoolTaskKind::Call(task),
        }
    }

    pub fn inspect(task: InspectionTask, permit: Permit) -> Self {
        Self {
            span: Span::current(),
            kind: EvmKind::Inspect,
            permit,
            task: PoolTaskKind::Inspect(task),
        }
    }

    pub fn execute(self, evm: &mut Evm) -> anyhow::Result<(), StratusError> {
        let Self {
            span,
            kind,
            permit: _permit,
            task,
        } = self;
        let _enter = span.enter();
        let _busy = kind.mark_executor_pool_busy();

        catch_unwind(AssertUnwindSafe(move || match task {
            PoolTaskKind::Call(task) => task.execute(evm),
            PoolTaskKind::Inspect(task) => task.execute(evm),
        }))
        .map_err(|err| ExecutorError::Panic { err: anyhow!("{err:?}") }.into())
    }
}

impl<Input: EvmInput> ExecutionTask<Input> {
    fn execute(self, evm: &mut Evm) {
        if let Err(e) = self.response_tx.send(evm.execute(self.input)) {
            tracing::error!(reason = ?e, "failed to send evm task execution result");
        }
    }
}

impl InspectionTask {
    fn execute(self, evm: &mut Evm) {
        if let Err(e) = self.response_tx.send(evm.inspect(self.input)) {
            tracing::error!(reason = ?e, "failed to send evm task execution result");
        }
    }
}
