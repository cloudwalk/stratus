use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use serde_json::json;

use super::importer::ImporterMode;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::Importer;
use crate::eth::miner::Miner;
use crate::eth::primitives::ConsensusError;
use crate::eth::primitives::ImporterError;
use crate::eth::primitives::StateError;
use crate::eth::primitives::StratusError;
use crate::eth::rpc::RpcContext;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::parse_duration;
use crate::ext::spawn_named;
use crate::infra::kafka::KafkaConnector;
use crate::infra::BlockchainClient;
use crate::GlobalState;
use crate::NodeMode;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize)]
#[group(requires_all = ["external_rpc", "follower"])]
pub struct ImporterConfig {
    /// External RPC HTTP endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC", required = false)]
    pub external_rpc: String,

    /// External RPC WS endpoint to sync blocks with Stratus.
    #[arg(short = 'w', long = "external-rpc-ws", env = "EXTERNAL_RPC_WS", required = false)]
    pub external_rpc_ws: Option<String>,

    /// Timeout for blockchain requests (importer online)
    #[arg(long = "external-rpc-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_TIMEOUT", default_value = "2s", required = false)]
    pub external_rpc_timeout: Duration,

    /// Maximum response size in bytes for external RPC requests
    #[arg(
        long = "external-rpc-max-response-size-bytes",
        env = "EXTERNAL_RPC_MAX_RESPONSE_SIZE_BYTES",
        default_value = "10485760"
    )]
    pub external_rpc_max_response_size_bytes: u32,

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "100ms", required = false)]
    pub sync_interval: Duration,
}

impl ImporterConfig {
    pub async fn init(
        &self,
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        kafka_connector: Option<KafkaConnector>,
    ) -> anyhow::Result<Option<Arc<Importer>>> {
        match GlobalState::get_node_mode() {
            NodeMode::Leader => Ok(None),
            NodeMode::Follower =>
                self.init_follower(executor, miner, storage, kafka_connector, ImporterMode::NormalFollower)
                    .await,
            NodeMode::FakeLeader => self.init_follower(executor, miner, storage, kafka_connector, ImporterMode::FakeLeader).await,
        }
    }

    async fn init_follower(
        &self,
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        kafka_connector: Option<KafkaConnector>,
        importer_mode: ImporterMode,
    ) -> anyhow::Result<Option<Arc<Importer>>> {
        const TASK_NAME: &str = "importer::init";
        tracing::info!("creating importer for follower node");

        let importer_mode = if storage.rocksdb_replication_enabled() {
            ImporterMode::RocksDbReplication
        } else {
            importer_mode
        };

        let chain = Arc::new(
            BlockchainClient::new_http_ws(
                &self.external_rpc,
                self.external_rpc_ws.as_deref(),
                self.external_rpc_timeout,
                self.external_rpc_max_response_size_bytes,
            )
            .await?,
        );

        let importer = Importer::new(
            executor,
            Arc::clone(&miner),
            Arc::clone(&storage),
            Arc::clone(&chain),
            kafka_connector.map(Arc::new),
            self.sync_interval,
            importer_mode,
        );
        let importer = Arc::new(importer);

        spawn_named(TASK_NAME, {
            let importer = Arc::clone(&importer);
            async move {
                if let Err(e) = importer.run_importer_online().await {
                    tracing::error!(reason = ?e, "importer-online failed");
                }
            }
        });

        Ok(Some(importer))
    }

    pub async fn init_follower_importer(&self, ctx: Arc<RpcContext>) -> Result<serde_json::Value, StratusError> {
        if GlobalState::get_node_mode() != NodeMode::Follower {
            tracing::error!("node is currently not a follower");
            return Err(StateError::StratusNotFollower.into());
        }

        if not(GlobalState::is_importer_shutdown()) {
            tracing::error!("importer is already running");
            return Err(ImporterError::AlreadyRunning.into());
        }

        GlobalState::set_importer_shutdown(false);

        let consensus = match self
            .init(Arc::clone(&ctx.executor), Arc::clone(&ctx.miner), Arc::clone(&ctx.storage), None)
            .await
        {
            Ok(consensus) => consensus,
            Err(e) => {
                tracing::error!(reason = ?e, "failed to initialize importer");
                GlobalState::set_importer_shutdown(true);
                return Err(ImporterError::InitError.into());
            }
        };

        match consensus {
            Some(consensus) => {
                ctx.set_importer(Some(consensus));
            }
            None => {
                tracing::error!("failed to update consensus: Consensus is not set.");
                GlobalState::set_importer_shutdown(true);
                return Err(ConsensusError::NotSet.into());
            }
        }

        Ok(json!(true))
    }
}
