use std::sync::Arc;

use super::Server;
use crate::eth::rpc::rpc_subscriptions::RpcSubscriptionsConnected;

pub struct RpcContext {
    pub server: Arc<Server>,
    pub client_version: &'static str,
    pub gas_price: usize,
    pub subs: Arc<RpcSubscriptionsConnected>,
}
