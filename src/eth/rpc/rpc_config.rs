use std::net::SocketAddr;

use clap::Parser;
use display_json::DebugAsJson;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct RpcServerConfig {
    /// JSON-RPC binding address.
    #[arg(short = 'a', long = "address", env = "ADDRESS", default_value = "0.0.0.0:3000")]
    pub address: SocketAddr,

    /// JSON-RPC max active connections
    #[arg(long = "max-connections", env = "MAX_CONNECTIONS", default_value = "200")]
    pub max_connections: u32,

    /// JSON-RPC max active subscriptions per client.
    #[arg(long = "max-subscriptions", env = "MAX_SUBSCRIPTIONS", default_value = "15")]
    pub max_subscriptions: u32,
}
