use std::fmt::Debug;
use std::sync::Arc;

use crate::alias::JsonValue;
use crate::eth::executor::Executor;
use crate::eth::miner::Miner;
use crate::eth::primitives::ChainId;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::StratusStorage;
use crate::eth::Consensus;

pub struct RpcContext {
    // app config
    pub app_config: JsonValue,

    // blockchain config
    pub chain_id: ChainId,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // services
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
    pub consensus: Arc<Consensus>,
    pub rpc_server: RpcServerConfig,
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
