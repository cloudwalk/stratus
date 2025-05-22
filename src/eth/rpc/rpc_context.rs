use std::sync::Arc;

use parking_lot::RwLock;

use crate::alias::JsonValue;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::Importer;
use crate::eth::miner::Miner;
use crate::eth::primitives::ChainId;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;
use crate::eth::rpc::RpcServerConfig;
use crate::eth::storage::StratusStorage;

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
    pub importer: RwLock<Option<Arc<Importer>>>,
    pub rpc_server: RpcServerConfig,
    pub subs: Arc<RpcSubscriptionsConnected>,
}

impl RpcContext {
    pub fn importer(&self) -> Option<Arc<Importer>> {
        self.importer.read().clone()
    }

    pub fn set_importer(&self, new_consensus: Option<Arc<Importer>>) {
        *self.importer.write() = new_consensus;
    }
}
