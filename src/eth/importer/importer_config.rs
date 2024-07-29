use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::executor::Executor;
use crate::eth::importer::Importer;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;
use crate::ext::parse_duration;
use crate::ext::spawn_named;
use crate::infra::BlockchainClient;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize)]
#[group(requires_all = ["external_rpc"])]
pub struct ImporterConfig {
    /// External RPC HTTP endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC", required = false)]
    pub external_rpc: String,

    /// External RPC WS endpoint to sync blocks with Stratus.
    #[arg(short = 'w', long = "external-rpc-ws", env = "EXTERNAL_RPC_WS")]
    pub external_rpc_ws: Option<String>,

    /// Timeout for blockchain requests (importer online)
    #[arg(long = "external-rpc-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_TIMEOUT", default_value = "2s")]
    pub external_rpc_timeout: Duration,

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "100ms")]
    pub sync_interval: Duration,
}

impl ImporterConfig {
    pub fn init(
        &self,
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        chain: Arc<BlockchainClient>,
    ) -> anyhow::Result<Arc<Importer>> {
        const TASK_NAME: &str = "importer::init";
        tracing::info!(config = ?self, "creating importer");

        let config = self.clone();

        let importer = Importer::new(executor, miner, Arc::clone(&storage), chain, config.sync_interval);
        let importer = Arc::new(importer);

        spawn_named(TASK_NAME, {
            let importer = Arc::clone(&importer);
            async move {
                if let Err(e) = importer.run_importer_online().await {
                    tracing::error!(reason = ?e, "importer-online failed");
                }
            }
        });

        Ok(importer)
    }
}
