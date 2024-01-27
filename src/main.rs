use std::cmp::max;
use std::sync::Arc;
use std::thread;
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
    let runtime = init_async_runtime();
    runtime.block_on(run_rpc_server())
}

pub fn init_async_runtime() -> Runtime {
    Builder::new_multi_thread()
        .enable_all()
        .thread_name("tokio")
        .worker_threads(1)
        .max_blocking_threads(1)
        .thread_keep_alive(Duration::from_secs(u64::MAX))
        .build()
        .expect("failed to build tokio runtime")
}

async fn run_rpc_server() -> anyhow::Result<()> {
    // parse cli configs
    let config = Config::parse();

    // init infra
    infra::init_tracing();
    infra::init_metrics();

    // init services
    let storage: Arc<dyn EthStorage> = match config.storage {
        StorageConfig::InMemory => Arc::new(InMemoryStorage::default().metrified()),
        StorageConfig::Postgres { url } => Arc::new(Postgres::new(&url).await?.metrified()),
    };

    // init executor
    let evms = init_evms(Arc::clone(&storage));
    let executor = EthExecutor::new(evms, Arc::clone(&storage));

    // start rpc server
    serve_rpc(executor, storage, config.address).await?;
    Ok(())
}

/// Inits EVMs that will executes transactions in parallel.
///
/// TODO: The number of EVMs may be configurable instead of assuming a value based on number of processors.
fn init_evms(storage: Arc<dyn EthStorage>) -> NonEmpty<Box<dyn Evm>> {
    // calculate the number of EVMs
    let cpu_cores = thread::available_parallelism().unwrap();
    let num_evms = max(1, cpu_cores.get() - 2); // TODO: make this value configurable

    // create EVMs
    let mut evms: Vec<Box<dyn Evm>> = Vec::with_capacity(10);
    for _ in 1..=num_evms {
        evms.push(Box::new(Revm::new(Arc::clone(&storage))));
    }
    tracing::info!(evms = %num_evms, "evms initialized");
    NonEmpty::from_vec(evms).unwrap()
}
