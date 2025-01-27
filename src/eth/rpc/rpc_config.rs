use std::collections::HashSet;
use std::net::SocketAddr;

use anyhow::anyhow;
use clap::Parser;

use super::RpcClientApp;

#[derive(Parser, Clone, serde::Serialize)]
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

    #[arg(long = "rpc-debug-trace-unsuccessful-only", value_parser=Self::parse_rpc_client_app_hashset ,env = "RPC_DEBUG_TRACE_UNSUCCESSFUL_ONLY")]
    pub rpc_debug_trace_unsuccessful_only: Option<HashSet<RpcClientApp>>,
}

impl RpcServerConfig {
    pub fn parse_rpc_client_app_hashset(input: &str) -> anyhow::Result<HashSet<RpcClientApp>> {
        if input.is_empty() {
            return Err(anyhow!("invalid client list"));
        }

        let set: HashSet<RpcClientApp> = input.split(',').map(|s| RpcClientApp::parse(s.trim())).collect();

        if set.is_empty() {
            Err(anyhow!("invalid client list"))
        } else {
            Ok(set)
        }
    }
}
