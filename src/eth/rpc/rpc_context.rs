use std::fmt::Debug;
use std::sync::Arc;

use crate::eth::primitives::ChainId;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::eth::Executor;

pub struct RpcContext {
    // blockchain config
    pub chain_id: ChainId,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // services
    pub executor: Arc<Executor>,
    pub miner: Arc<BlockMiner>,
    pub storage: Arc<StratusStorage>,
    pub subs: Arc<RpcSubscriptionsConnected>,
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
