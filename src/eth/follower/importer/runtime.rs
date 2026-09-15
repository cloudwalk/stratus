use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Builder;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::eth::executor::Executor;
use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::ImporterMode;
use crate::eth::follower::importer::supervisor::start_importer;
use crate::eth::miner::Miner;
use crate::eth::storage::StratusStorage;
use crate::eth::types::BlockNumber;
use crate::infra::kafka::KafkaConnector;

/// Owns the thread on which the importer's dedicated Tokio runtime runs.
///
/// Dropping the handle detaches the thread. The importer itself is stopped through the existing
/// global importer/application shutdown signals, just like the previous main-runtime task.
pub struct ImporterRuntime {
    shutdown: CancellationToken,
    thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

pub struct ImporterRuntimeConfig {
    pub async_threads: usize,
    pub importer_mode: ImporterMode,
    pub external_rpc: String,
    pub external_rpc_ws: Option<String>,
    pub external_rpc_timeout: Duration,
    pub sync_interval: Duration,
    pub stop_at_block: Option<BlockNumber>,
    pub storage: Arc<StratusStorage>,
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

impl ImporterRuntime {
    /// Starts an importer on a dedicated runtime and waits until its HTTP/WS client has been
    /// constructed and has successfully reached the leader.
    pub async fn start(config: ImporterRuntimeConfig) -> anyhow::Result<Self> {
        let (initialized_tx, initialized_rx) = oneshot::channel::<Result<(), String>>();
        let shutdown = CancellationToken::new();
        let runtime_shutdown = shutdown.clone();

        let thread = std::thread::Builder::new()
            .name("importer-runtime".to_owned())
            .spawn(move || run_importer_runtime(config, runtime_shutdown, initialized_tx))?;

        match initialized_rx.await {
            Ok(Ok(())) => Ok(Self {
                shutdown,
                thread: Some(thread),
            }),
            Ok(Err(reason)) => {
                let _ = thread.join();
                anyhow::bail!("failed to initialize dedicated importer runtime: {reason}")
            }
            Err(_) => {
                let result = thread.join();
                anyhow::bail!("dedicated importer runtime stopped during initialization: {result:?}")
            }
        }
    }

    /// Requests shutdown and asynchronously joins the runtime owner thread.
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.shutdown.cancel();
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || match thread.join() {
            Ok(result) => result,
            Err(panic) => anyhow::bail!("dedicated importer runtime panicked: {panic:?}"),
        })
        .await?
    }
}

impl Drop for ImporterRuntime {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

fn run_importer_runtime(config: ImporterRuntimeConfig, shutdown: CancellationToken, initialized_tx: oneshot::Sender<Result<(), String>>) -> anyhow::Result<()> {
    let runtime = match Builder::new_multi_thread()
        .enable_all()
        .worker_threads(config.async_threads)
        .thread_name("tokio-importer")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let reason = format!("failed to build importer Tokio runtime: {error:#}");
            let _ = initialized_tx.send(Err(reason.clone()));
            anyhow::bail!(reason);
        }
    };

    runtime.block_on(async move {
        let chain = match BlockchainClient::new_http_ws(&config.external_rpc, config.external_rpc_ws.as_deref(), config.external_rpc_timeout).await {
            Ok(chain) => Arc::new(chain),
            Err(error) => {
                let reason = format!("failed to create importer blockchain client: {error:#}");
                let _ = initialized_tx.send(Err(reason.clone()));
                anyhow::bail!(reason);
            }
        };

        let _ = initialized_tx.send(Ok(()));
        tokio::select! {
            result = start_importer(
                config.importer_mode,
                config.storage,
                config.executor,
                config.miner,
                chain,
                config.kafka_connector,
                config.sync_interval,
                config.stop_at_block,
            ) => result,
            _ = shutdown.cancelled() => Ok(()),
        }
    })
}
