use revm::Database;

use crate::eth::executor::evm::GeneralRevm;
use crate::eth::executor::evm::types::ExecutionMetricsContext;
use crate::eth::types::ExecutionContext;

pub mod call_execution;
pub mod inspector;
pub mod transaction_execution;

pub trait EvmInput: Default + Clone {
    fn context(&self) -> ExecutionContext;

    fn metrics_context(&self) -> ExecutionMetricsContext;

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>);

    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>);

    fn fill_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        self.fill_block_env(evm);
        self.fill_tx_env(evm);
    }
}
