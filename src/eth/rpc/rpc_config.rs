use std::net::SocketAddr;

use clap::Parser;
use display_json::DebugAsJson;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct RpcServerConfig {
    /// JSON-RPC server binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub rpc_address: SocketAddr,

    /// JSON-RPC server max active connections
    #[arg(long = "max-connections", env = "MAX_CONNECTIONS", default_value = "400")]
    pub rpc_max_connections: u32,

    /// JSON-RPC server max active subscriptions per client.
    #[arg(long = "max-subscriptions", env = "MAX_SUBSCRIPTIONS", default_value = "30")]
    pub rpc_max_subscriptions: u32,
}
