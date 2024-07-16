use std::fmt::Debug;
use std::sync::Arc;

use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::primitives::ChainId;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;
use crate::eth::storage::StratusStorage;
use crate::eth::Consensus;

pub struct RpcContext {
    // blockchain config
    pub chain_id: ChainId,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // general config
    pub max_subscriptions: u32,

    // services
    pub executor: Arc<Executor>,
    #[allow(dead_code)] // HACK this was triggered in Rust 1.79
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
    pub consensus: Arc<Consensus>,
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
