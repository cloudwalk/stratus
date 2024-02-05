use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use nonempty::NonEmpty;
use stratus::config::Config;
use stratus::config::StorageConfig;
use stratus::eth::evm::revm::Revm;
use stratus::eth::evm::Evm;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::storage::EthStorage;
use stratus::eth::storage::InMemoryStorage;
use stratus::eth::EthExecutor;
use stratus::infra;
use stratus::infra::postgres::Postgres;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

fn main() -> anyhow::Result<()> {
    let config = Arc::new(Config::parse());

    infra::init_tracing();
    infra::init_metrics();

    let config_clone_for_rpc = Arc::clone(&config);

    let runtime = init_async_runtime(&config);

    runtime.block_on(run_rpc_server(config_clone_for_rpc))
}

pub fn init_async_runtime(config: &Config) -> Runtime {
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .thread_name("tokio")
        .worker_threads(config.num_async_threads)
        .max_blocking_threads(config.num_blocking_threads)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .build()
        .expect("failed to build tokio runtime");

    tracing::info!(
        async_threads = %config.num_async_threads,
        blocking_threads = %config.num_blocking_threads,
        "async runtime initialized"
    );

    runtime
}

async fn run_rpc_server(config: Arc<Config>) -> anyhow::Result<()> {
    tracing::info!("Starting RPC server");

    let storage: Arc<dyn EthStorage> = match &config.storage {
        StorageConfig::InMemory => Arc::new(InMemoryStorage::default().metrified()),
        StorageConfig::Postgres { url } => Arc::new(Postgres::new(url).await?.metrified()),
    };

    // init executor
    let evms = init_evms(&config, Arc::clone(&storage));
    let executor = EthExecutor::new(evms, Arc::clone(&storage));

    serve_rpc(executor, storage, config.address).await?;

    tracing::info!("RPC server started");
    Ok(())
}

/// Inits EVMs that will executes transactions in parallel.
fn init_evms(config: &Config, storage: Arc<dyn EthStorage>) -> NonEmpty<Box<dyn Evm>> {
    let mut evms: Vec<Box<dyn Evm>> = Vec::with_capacity(config.num_evms);
    for _ in 1..=config.num_evms {
        evms.push(Box::new(Revm::new(Arc::clone(&storage))));
    }
    tracing::info!(evms = %config.num_evms, "evms initialized");
    NonEmpty::from_vec(evms).unwrap()
}
