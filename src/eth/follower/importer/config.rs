use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use serde_json::json;

use crate::GlobalState;
use crate::NodeMode;
use crate::eth::executor::Executor;
use crate::eth::follower::ConsensusError;
use crate::eth::follower::ImporterError;
use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::ImporterMode;
use crate::eth::follower::importer::ImporterRuntime;
use crate::eth::follower::importer::ImporterRuntimeConfig;
use crate::eth::follower::importer::supervisor::ImporterConsensus;
use crate::eth::miner::Miner;
use crate::eth::rpc::RpcContext;
use crate::eth::storage::StratusStorage;
use crate::eth::types::BlockNumber;
use crate::eth::types::StateError;
use crate::eth::types::StratusError;
use crate::ext::duration_serde;
use crate::ext::not;
use crate::ext::parse_duration;
use crate::infra::kafka::KafkaConnector;

#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct ImporterConfig {
    /// External RPC HTTP endpoint to sync blocks with Stratus. Empty by default; `validate()` rejects it after the merge.
    #[arg(id = "importer.external_rpc", short = 'r', long = "external-rpc", default_value = "", required = false)]
    pub external_rpc: String,

    /// External RPC WS endpoint to sync blocks with Stratus.
    #[arg(id = "importer.external_rpc_ws", short = 'w', long = "external-rpc-ws", required = false)]
    pub external_rpc_ws: Option<String>,

    /// Timeout for blockchain requests (importer online)
    #[arg(
        id = "importer.external_rpc_timeout",
        long = "external-rpc-timeout",
        value_parser = parse_duration,
        default_value = "2s",
        required = false
    )]
    #[serde(with = "duration_serde")]
    pub external_rpc_timeout: Duration,

    #[arg(id = "importer.sync_interval", long = "sync-interval", value_parser = parse_duration, default_value = "100ms", required = false)]
    #[serde(with = "duration_serde")]
    pub sync_interval: Duration,

    /// Enable replication of block changes
    #[arg(
        id = "importer.enable_block_changes_replication",
        long = "enable-block-changes-replication",
        default_value = "false"
    )]
    pub enable_block_changes_replication: bool,

    /// Number of Tokio worker threads dedicated to the online importer.
    #[arg(id = "importer.async_threads", long = "importer-async-threads", default_value = "4", required = false)]
    pub importer_async_threads: usize,

    /// Compute an access list for transactions before forwarding them to the leader.
    #[arg(
        id = "importer.forward_access_list",
        long = "forward-access-list",
        default_value = "true",
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        required = false
    )]
    pub forward_access_list: bool,

    /// Specify the block to stop importing. (useful for validating a follower db against a fake leader)
    #[arg(id = "importer.stop_at_block", long = "stop-at-block")]
    pub stop_at_block: Option<BlockNumber>,
}

impl Default for ImporterConfig {
    fn default() -> Self {
        Self {
            external_rpc: String::new(),
            external_rpc_ws: None,
            external_rpc_timeout: Duration::from_millis(2_000),
            sync_interval: Duration::from_millis(100),
            enable_block_changes_replication: false,
            importer_async_threads: 4,
            forward_access_list: true,
            stop_at_block: None,
        }
    }
}

impl ImporterConfig {
    pub async fn init(
        &self,
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        kafka_connector: Option<KafkaConnector>,
    ) -> anyhow::Result<Option<(Arc<ImporterConsensus>, ImporterRuntime)>> {
        match GlobalState::get_node_mode() {
            NodeMode::Leader => Ok(None),
            NodeMode::Follower => self
                .init_follower(
                    executor,
                    miner,
                    storage,
                    kafka_connector,
                    if self.enable_block_changes_replication {
                        ImporterMode::BlockWithChanges
                    } else {
                        ImporterMode::ReexecutionFollower
                    },
                )
                .await
                .map(Some),
            NodeMode::FakeLeader => self
                .init_follower(executor, miner, storage, kafka_connector, ImporterMode::FakeLeader)
                .await
                .map(Some),
        }
    }

    async fn init_follower(
        &self,
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        kafka_connector: Option<KafkaConnector>,
        importer_mode: ImporterMode,
    ) -> anyhow::Result<(Arc<ImporterConsensus>, ImporterRuntime)> {
        tracing::info!(importer_async_threads = self.importer_async_threads, "creating importer for follower node");

        // Forwarding stays on the RPC runtime and uses an independent Hyper connection pool.
        let forwarding_chain = Arc::new(BlockchainClient::new_http(&self.external_rpc, self.external_rpc_timeout).await?);
        let consensus = Arc::new(ImporterConsensus {
            storage: Arc::clone(&storage),
            chain: forwarding_chain,
            executor: Arc::clone(&executor),
            forward_access_list: self.forward_access_list,
        });

        let importer_runtime = ImporterRuntime::start(ImporterRuntimeConfig {
            async_threads: self.importer_async_threads,
            importer_mode,
            external_rpc: self.external_rpc.clone(),
            external_rpc_ws: self.external_rpc_ws.clone(),
            external_rpc_timeout: self.external_rpc_timeout,
            sync_interval: self.sync_interval,
            stop_at_block: self.stop_at_block,
            storage,
            executor,
            miner,
            kafka_connector,
        })
        .await?;

        Ok((consensus, importer_runtime))
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
            .init(
                Arc::clone(&ctx.server.executor),
                Arc::clone(&ctx.server.miner),
                Arc::clone(&ctx.server.storage),
                None,
            )
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
            Some((consensus, importer_runtime)) => {
                ctx.server.set_importer(Some(consensus));
                ctx.server.set_importer_runtime(Some(importer_runtime));
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
