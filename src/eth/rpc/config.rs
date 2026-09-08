use std::collections::HashSet;
use std::net::SocketAddr;

use anyhow::bail;
use clap::Parser;
use display_json::DebugAsJson;
use stratus_macros::CliOverrides;

use crate::eth::rpc::RpcClientApp;
use crate::eth::rpc::pagination;

#[derive(Parser, Clone, DebugAsJson, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct RpcServerConfig {
    /// JSON-RPC server binding address.
    #[arg(short = 'a', long = "address", default_value = "0.0.0.0:3000")]
    #[serde(rename = "address")]
    pub rpc_address: SocketAddr,

    /// JSON-RPC server max active connections
    #[arg(long = "max-connections", default_value = "400")]
    #[serde(rename = "max_connections")]
    pub rpc_max_connections: u32,

    /// JSON-RPC server max response size limit in bytes
    #[arg(
        long = "max-response-size-bytes",
        default_value = "10485760",
        value_parser = Self::parse_max_response_size_bytes
    )]
    #[serde(rename = "max_response_size_bytes")]
    pub rpc_max_response_size_bytes: u32,

    /// JSON-RPC server max active subscriptions per client.
    #[arg(long = "max-subscriptions", default_value = "30")]
    #[serde(rename = "max_subscriptions")]
    pub rpc_max_subscriptions: u32,

    /// Health check interval in seconds
    #[arg(long = "health-check-interval", default_value = "100")]
    pub health_check_interval_ms: u64,

    /// JSON-RPC server max batch request limit
    #[arg(long = "batch-request-limit", default_value = "500")]
    pub batch_request_limit: u32,

    /// Debug trace endpoints that should return only unsuccessful transactions for the given client apps.
    #[arg(long = "rpc-debug-trace-unsuccessful-only", value_parser = Self::parse_rpc_client_app_hashset)]
    #[serde(rename = "debug_trace_unsuccessful_only")]
    pub rpc_debug_trace_unsuccessful_only: Option<HashSet<RpcClientApp>>,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            rpc_address: SocketAddr::from(([0, 0, 0, 0], 3000)),
            rpc_max_connections: 400,
            rpc_max_response_size_bytes: 10485760,
            rpc_max_subscriptions: 30,
            health_check_interval_ms: 100,
            batch_request_limit: 500,
            rpc_debug_trace_unsuccessful_only: None,
        }
    }
}

impl RpcServerConfig {
    /// Parses a response size limit, rejecting values too small for importer pagination.
    fn parse_max_response_size_bytes(input: &str) -> Result<u32, String> {
        let value = input.parse::<u32>().map_err(|error| error.to_string())?;
        if value < pagination::MIN_RESPONSE_SIZE_BYTES {
            return Err(format!(
                "must be at least {} bytes, otherwise importer pagination cannot fit a chunk",
                pagination::MIN_RESPONSE_SIZE_BYTES
            ));
        }
        Ok(value)
    }

    pub fn parse_rpc_client_app_hashset(input: &str) -> anyhow::Result<HashSet<RpcClientApp>> {
        if input.is_empty() {
            bail!("invalid client list");
        }

        let set: HashSet<RpcClientApp> = input.split(',').map(|s| RpcClientApp::parse(s.trim())).collect();

        if set.is_empty() { bail!("invalid client list") } else { Ok(set) }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::eth::rpc::pagination::MIN_RESPONSE_SIZE_BYTES;

    #[test]
    fn rpc_max_response_size_bytes_must_be_at_least_the_pagination_floor() {
        // below the floor: pagination could not fit a chunk in a response
        let error = RpcServerConfig::try_parse_from(["stratus", "--max-response-size-bytes", "300"]).expect_err("below the floor should be rejected");
        assert!(error.to_string().contains("must be at least 704 bytes"));

        // at the floor and above: accepted
        RpcServerConfig::try_parse_from(["stratus", "--max-response-size-bytes", &MIN_RESPONSE_SIZE_BYTES.to_string()]).expect("floor should be accepted");
        RpcServerConfig::try_parse_from(["stratus", "--max-response-size-bytes", "10485760"]).expect("default should be accepted");
    }
}
