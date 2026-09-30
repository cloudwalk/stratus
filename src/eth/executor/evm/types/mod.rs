use revm::Context;
use revm::Journal;
use revm::context::BlockEnv;
use revm::context::CfgEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TxEnv;
use revm::handler::EthFrame;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::interpreter::interpreter::EthInterpreter;

mod execution_metrics;
mod input;
mod output;

pub use execution_metrics::ExecutionMetrics;
pub use execution_metrics::ExecutionMetricsContext;
pub use execution_metrics::StorageMetrics;
pub use input::EvmInput;
pub use input::call_execution::CallExecutionInput;
pub use input::inspector::InspectorInput;
pub use input::transaction_execution::TransactionExecutionInput;
pub use output::access_list::AccessListOutput;
pub use output::call_execution::CallExecutionOutput;
pub use output::transaction_execution::TransactionExecutionOutput;
pub use output::transaction_execution::TransactionExecutionResult;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
#[cfg(feature = "dev")]
pub const GAS_MAX_LIMIT: u64 = 1_000_000_000;
#[cfg(not(feature = "dev"))]
pub const GAS_MAX_LIMIT: u64 = 100_000_000;

pub type ContextWithDB<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>>;
pub type GeneralRevm<DB, I = ()> = RevmEvm<ContextWithDB<DB>, I, EthInstructions<EthInterpreter, ContextWithDB<DB>>, EthPrecompiles, EthFrame>;

/// Executor worker pool lane. Determines which pool executes the task and
/// labels the `executor_workers_busy` gauge.
#[derive(Clone, Copy)]
pub enum Lane {
    Transaction,
    CallPresent,
    CallPast,
    Inspector,
}
