use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

use crate::eth::evm::Evm;
use crate::eth::evm::EvmStorage;

pub struct RpcContext {
    // blockchain config
    pub chain_id: u16,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // services
    pub evm: Box<Mutex<dyn Evm>>,
    pub evm_storage: Arc<dyn EvmStorage>,
}

impl Debug for RpcContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcContext")
            .field("chain_id", &self.chain_id)
            .field("client_version", &self.client_version)
            .field("gas_price", &self.gas_price)
            .finish_non_exhaustive()
    }
}
