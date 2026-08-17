use revm::Database;

use crate::eth::executor::evm::GeneralRevm;
use crate::eth::types::ExecutionKind;

pub mod call_execution;
pub mod inspector;
pub mod transaction_execution;

pub trait EvmInput: Default + Clone {
    fn kind(&self) -> ExecutionKind;

    fn fill_tx_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>);

    fn fill_block_env<DB: Database, I>(&self, evm: &mut GeneralRevm<DB, I>);

    fn fill_env<DB: Database, I>(self, evm: &mut GeneralRevm<DB, I>) {
        self.fill_block_env(evm);
        self.fill_tx_env(evm);
    }
}
