use std::sync::Arc;

use derive_more::Debug;

use super::Server;
use crate::eth::rpc::subscriptions::RpcSubscriptionsConnected;

#[derive(Debug)]
pub struct RpcContext {
    #[debug(skip)]
    pub server: Arc<Server>,
    pub client_version: &'static str,
    pub subs: Arc<RpcSubscriptionsConnected>,
}
