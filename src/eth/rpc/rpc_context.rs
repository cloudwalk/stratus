use std::fmt::Debug;
use std::sync::Arc;

use crate::eth::rpc::RpcSubscriptions;
use crate::eth::storage::StratusStorage;
use crate::eth::EthExecutor;

pub struct RpcContext {
    // blockchain config
    pub chain_id: u16,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // services
    pub executor: EthExecutor,
    pub storage: Arc<StratusStorage>,
    pub subs: Arc<RpcSubscriptions>,
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
