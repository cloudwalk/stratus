use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;

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
use crate::eth::executor::pool_admission::PoolAdmission;
use crate::eth::executor::pool_admission::PoolPermit;
use crate::eth::executor::types::error::ExecutorError;
use crate::eth::types::PointInTime;
use crate::eth::types::StateError;
use crate::eth::types::StratusError;

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

/// A task for the unified EVM pool.
pub struct PoolTask {
    span: Span,
    permit: PoolPermit,
    task: PoolTaskKind,
}

enum PoolTaskKind {
    Call(ExecutionTask<CallExecutionInput>),
    Inspect(InspectionTask),
}

impl PoolTask {
    /// Creates a call task, acquiring an admission slot for it. Fails when the pool is shutting down.
    pub fn call(task: ExecutionTask<CallExecutionInput>, admission: &Arc<PoolAdmission>) -> Result<Self, StateError> {
        let permit = admission.acquire(call_evm_kind(&task.input))?;
        Ok(Self {
            span: Span::current(),
            permit,
            task: PoolTaskKind::Call(task),
        })
    }

    /// Creates an inspection task, acquiring an admission slot for it. Fails when the pool is shutting down.
    pub fn inspect(task: InspectionTask, admission: &Arc<PoolAdmission>) -> Result<Self, StateError> {
        let permit = admission.acquire(EvmKind::Inspect)?;
        Ok(Self {
            span: Span::current(),
            permit,
            task: PoolTaskKind::Inspect(task),
        })
    }

    /// Executes the task on the EVM. The admission slot is released when the task finishes.
    pub fn execute(self, evm: &mut Evm) -> anyhow::Result<(), StratusError> {
        let Self { span, permit, task } = self;
        let _enter = span.enter();
        let _busy = permit.evm_kind().mark_executor_pool_busy();

        catch_unwind(AssertUnwindSafe(move || match task {
            PoolTaskKind::Call(task) => task.execute(evm),
            PoolTaskKind::Inspect(task) => task.execute(evm),
        }))
        .map_err(|err| ExecutorError::Panic { err: anyhow!("{err:?}") }.into())
    }
}

/// Returns the pool kind of a call: calls against the latest state and calls against a past state
/// are admitted by different pool gates.
fn call_evm_kind(input: &CallExecutionInput) -> EvmKind {
    match input.kind.point_in_time() {
        PointInTime::Pending | PointInTime::Latest => EvmKind::CallPresent,
        PointInTime::Past(_) => EvmKind::CallPast,
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
