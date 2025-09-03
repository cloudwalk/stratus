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

    /// JSON-RPC server max response size limit in bytes
    #[arg(long = "max-response-size-bytes", env = "MAX_RESPONSE_SIZE_BYTES", default_value = "10485760")]
    pub rpc_max_response_size_bytes: u32,

    /// JSON-RPC server max active subscriptions per client.
    #[arg(long = "max-subscriptions", env = "MAX_SUBSCRIPTIONS", default_value = "30")]
    pub rpc_max_subscriptions: u32,

    /// Health check interval in seconds
    #[arg(long = "health-check-interval", env = "HEALTH_CHECK_INTERVAL_MS", default_value = "100")]
    pub health_check_interval_ms: u64,

    /// JSON-RPC server max batch request limit
    #[arg(long = "batch-request-limit", env = "BATCH_REQUEST_LIMIT", default_value = "500")]
    pub batch_request_limit: u32,

    #[arg(long = "rpc-debug-trace-unsuccessful-only", value_parser=Self::parse_rpc_client_app_hashset ,env = "RPC_DEBUG_TRACE_UNSUCCESSFUL_ONLY")]
    pub rpc_debug_trace_unsuccessful_only: Option<HashSet<RpcClientApp>>,
}

impl RpcServerConfig {
    pub fn parse_rpc_client_app_hashset(input: &str) -> anyhow::Result<HashSet<RpcClientApp>> {
        if input.is_empty() {
            return Err(anyhow!("invalid client list"));
        }

        let set: HashSet<RpcClientApp> = input.split(',').map(|s| RpcClientApp::parse(s.trim())).collect();

        if set.is_empty() { Err(anyhow!("invalid client list")) } else { Ok(set) }
    }
}
