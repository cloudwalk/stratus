use revm::Context;
use revm::Database;
use revm::Journal;
use revm::context::BlockEnv;
use revm::context::CfgEnv;
use revm::context::Evm as RevmEvm;
use revm::context::TxEnv;
use revm::handler::EthFrame;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::interpreter::interpreter::EthInterpreter;

use crate::eth::storage::ExecutionKind;

mod call_execution_input;
mod inspector_input;
mod transaction_execution_input;

pub use call_execution_input::CallExecutionInput;
pub use inspector_input::InspectorInput;
pub use transaction_execution_input::TransactionExecutionInput;

/// Maximum gas limit allowed for a transaction. Prevents a transaction from consuming too many resources.
#[cfg(feature = "dev")]
pub const GAS_MAX_LIMIT: u64 = 1_000_000_000;
#[cfg(not(feature = "dev"))]
pub const GAS_MAX_LIMIT: u64 = 100_000_000;

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

pub trait EvmInput: Default + Clone {
    fn kind(&self) -> ExecutionKind;

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>);

    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>);

    fn fill_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        self.fill_block_env(evm);
        self.fill_tx_env(evm);
    }
}
