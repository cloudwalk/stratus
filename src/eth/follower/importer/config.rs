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
use crate::ext::not;
use crate::ext::parse_duration;
use crate::infra::kafka::KafkaConnector;

fn parse_thread_count(value: &str) -> Result<usize, String> {
    let value = value.parse::<usize>().map_err(|error| error.to_string())?;
    if value == 0 {
        return Err("thread count must be greater than zero".to_owned());
    }
    Ok(value)
}

#[derive(Default, Parser, DebugAsJson, Clone, serde::Serialize)]
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

    #[arg(long = "sync-interval", value_parser=parse_duration, env = "SYNC_INTERVAL", default_value = "100ms", required = false)]
    pub sync_interval: Duration,

    /// Enable replication of block changes
    #[arg(long = "enable-block-changes-replication", env = "ENABLE_BLOCK_CHANGES_REPLICATION", default_value = "false")]
    pub enable_block_changes_replication: bool,

    /// Number of Tokio worker threads dedicated to the online importer.
    #[arg(
        long = "importer-async-threads",
        env = "IMPORTER_ASYNC_THREADS",
        default_value = "4",
        value_parser = parse_thread_count,
        required = false
    )]
    pub importer_async_threads: usize,

    /// Compute an access list for transactions before forwarding them to the leader.
    #[arg(long = "forward-access-list", env = "FORWARD_ACCESS_LIST", default_value = "true", required = false)]
    pub forward_access_list: bool,

    /// Specify the block to stop importing. (useful for validating a follower db against a fake leader)
    #[arg(long = "stop-at-block", env = "STOP_AT_BLOCK")]
    pub stop_at_block: Option<BlockNumber>,
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
            NodeMode::Follower =>
                self.init_follower(
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
    ) -> anyhow::Result<Option<(Arc<ImporterConsensus>, ImporterRuntime)>> {
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

        Ok(Some((consensus, importer_runtime)))
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
