use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use serde_json::json;

use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::follower::importer::Importer;
use crate::eth::miner::Miner;
use crate::eth::primitives::StratusError;
use crate::eth::rpc::RpcContext;
use crate::eth::storage::StratusStorage;
use crate::ext::not;
use crate::ext::parse_duration;
use crate::ext::spawn_named;
use crate::infra::kafka_config::KafkaConfig;
use crate::infra::kafka_connector::KafkaConnector;
use crate::infra::BlockchainClient;
use crate::GlobalState;
use crate::NodeMode;

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize, serde::Deserialize)]
#[group(requires_all = ["external_rpc"])]
#[clap(group = clap::ArgGroup::new("kafka").multiple(true).requires_all(&["kafka-bootstrap-servers", "kafka-topic", "kafka-client-id", "kafka-group-id"]))]
pub struct ImporterConfig {
    /// External RPC HTTP endpoint to sync blocks with Stratus.
    #[arg(short = 'r', long = "external-rpc", env = "EXTERNAL_RPC", conflicts_with("leader"))]
    pub external_rpc: String,

    /// External RPC WS endpoint to sync blocks with Stratus.
    #[arg(short = 'w', long = "external-rpc-ws", env = "EXTERNAL_RPC_WS", conflicts_with("leader"))]
    pub external_rpc_ws: Option<String>,

    /// Timeout for blockchain requests (importer online)
    #[arg(long = "external-rpc-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_TIMEOUT", default_value = "2s")]
    pub external_rpc_timeout: Duration,

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "100ms")]
    pub sync_interval: Duration,

    #[arg(long = "kafka-bootstrap-servers", env = "KAFKA_BOOTSTRAP_SERVERS", group = "kafka")]
    pub bootstrap_servers: Option<String>,

    #[arg(long = "kafka-topic", env = "KAFKA_TOPIC", group = "kafka")]
    pub topic: Option<String>,

    #[arg(long = "kafka-client-id", env = "KAFKA_CLIENT_ID", group = "kafka")]
    pub client_id: Option<String>,

    #[arg(long = "kafka-group-id", env = "KAFKA_GROUP_ID", group = "kafka")]
    pub group_id: Option<String>,
}

impl ImporterConfig {
    pub async fn init(&self, executor: Arc<Executor>, miner: Arc<Miner>, storage: Arc<StratusStorage>) -> anyhow::Result<Option<Arc<dyn Consensus>>> {
        match GlobalState::get_node_mode() {
            NodeMode::Follower => self.init_follower(executor, miner, storage).await,
            NodeMode::Leader => Ok(None),
        }
    }

    pub fn kafka_config(&self) -> KafkaConfig {
        KafkaConfig {
            bootstrap_servers: self.bootstrap_servers.clone(),
            topic: self.topic.clone(),
            client_id: self.client_id.clone(),
            group_id: self.group_id.clone(),
        }
    }

    pub fn has_kafka_config(&self) -> bool {
        self.bootstrap_servers.is_some() && self.topic.is_some() && self.client_id.is_some() && self.group_id.is_some()
    }

    async fn init_follower(&self, executor: Arc<Executor>, miner: Arc<Miner>, storage: Arc<StratusStorage>) -> anyhow::Result<Option<Arc<dyn Consensus>>> {
        const TASK_NAME: &str = "importer::init";
        tracing::info!(config = ?self, "creating importer for follower node");

        let chain = Arc::new(BlockchainClient::new_http_ws(&self.external_rpc, self.external_rpc_ws.as_deref(), self.external_rpc_timeout).await?);
        //TODO: add kafka connector
        let kafka_connector = if self.has_kafka_config() {
            tracing::info!("creating kafka connector");
            match KafkaConnector::new(&self.kafka_config()) {
                Ok(kafka_connector) => Some(Arc::new(kafka_connector)),
                Err(e) => {
                    tracing::error!(reason = ?e, "importer-online failed to create kafka connector");
                    GlobalState::shutdown_from(TASK_NAME, "failed to create kafka connector");
                    None
                }
            }
        } else {
            None
        };

        let importer = Importer::new(
            executor,
            Arc::clone(&miner),
            Arc::clone(&storage),
            Arc::clone(&chain),
            kafka_connector,
            self.sync_interval,
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
        if not(GlobalState::is_follower()) {
            tracing::error!("node is currently not a follower");
            return Err(StratusError::StratusNotFollower);
        }

        if not(GlobalState::is_importer_shutdown()) {
            tracing::error!("importer is already running");
            return Err(StratusError::ImporterAlreadyRunning);
        }

        GlobalState::set_importer_shutdown(false);

        let consensus = match self.init(Arc::clone(&ctx.executor), Arc::clone(&ctx.miner), Arc::clone(&ctx.storage)).await {
            Ok(consensus) => consensus,
            Err(e) => {
                tracing::error!(reason = ?e, "failed to initialize importer");
                GlobalState::set_importer_shutdown(true);
                return Err(StratusError::ImporterInitError);
            }
        };

        match consensus {
            Some(consensus) => {
                ctx.set_consensus(Some(consensus));
            }
            None => {
                tracing::error!("failed to update consensus: Consensus is not set.");
                GlobalState::set_importer_shutdown(true);
                return Err(StratusError::ConsensusNotSet);
            }
        }

        Ok(json!(true))
    }
}
