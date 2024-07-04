use std::fmt::Debug;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::eth::primitives::ChainId;
use crate::eth::rpc::rpc_client_app::RpcClientApp;
use crate::eth::rpc::rpc_client_error;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;
use crate::eth::rpc::RpcError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::eth::Consensus;
use crate::eth::Executor;

pub struct RpcContext {
    // blockchain config
    pub chain_id: ChainId,
    pub client_version: &'static str,

    // gas config
    pub gas_price: usize,

    // client config
    pub reject_unknown_client_enabled: AtomicBool,

    // services
    pub executor: Arc<Executor>,
    #[allow(dead_code)] // HACK this was triggered in Rust 1.79
    pub miner: Arc<BlockMiner>,
    pub storage: Arc<StratusStorage>,
    pub consensus: Arc<Consensus>,
    pub subs: Arc<RpcSubscriptionsConnected>,
}

impl RpcContext {
    /// Returns an error JSON-RPC response if the client is not allowed to perform the current operation.
    pub fn reject_unknown_client(&self, client: RpcClientApp) -> Result<(), RpcError> {
        let enabled = self.reject_unknown_client_enabled.load(Ordering::Relaxed);
        if enabled && client.is_unknown() {
            tracing::warn!("operation rejected because rpc client is unknown");
            return Err(rpc_client_error("operation rejected because client did not identify itself using url query parameter or headers").into());
        }
        Ok(())
    }
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
