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

pub type ContextWithDB<DB> = Context<BlockEnv, TxEnv, CfgEnv, DB, Journal<DB>>;
pub type GeneralRevm<DB, I = ()> = RevmEvm<ContextWithDB<DB>, I, EthInstructions<EthInterpreter, ContextWithDB<DB>>, EthPrecompiles, EthFrame>;

/// Classification of an EVM by the kind of execution it performs. Used to route
/// work to the right EVM worker pool and as a metrics label.
#[derive(Clone, Copy)]
pub enum EvmKind {
    Transaction,
    CallPast,
    CallPresent,
    Inspect,
}

impl EvmKind {
    pub fn is_call(&self) -> bool {
        match self {
            EvmKind::Transaction => false,
            EvmKind::CallPast | EvmKind::CallPresent | EvmKind::Inspect => true,
        }
    }

    pub fn is_transaction(&self) -> bool {
        !self.is_call()
    }
}
