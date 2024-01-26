use std::cmp::max;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::Parser;
use nonempty::NonEmpty;
use stratus::config;
use stratus::config::Config;
use stratus::config::StorageConfig;
use stratus::eth::evm::revm::Revm;
use stratus::eth::evm::Evm;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::storage::EthStorage;
use stratus::eth::storage::InMemoryStorage;
use stratus::eth::EthExecutor;
use stratus::ext::new_tokio_runtime;
use stratus::infra;
use stratus::infra::postgres::Postgres;
use tokio::task::JoinError;

fn main() -> anyhow::Result<()> {
    // parse cli configs
    let config = Config::parse();

    // init infra
    infra::init_tracing();
    infra::init_metrics();

    let runtime = new_tokio_runtime("tokio-main", 2, 2); // TODO: make this value configurable

    runtime.block_on(async {
        let rpc_handle = tokio::spawn(run_rpc_server(config));
        let p2p_handle = tokio::spawn(run_p2p_server());

        match tokio::try_join!(rpc_handle, p2p_handle) {
            Ok((rpc_result, p2p_result)) => {
                dbg!(&rpc_result);
                rpc_result?;
                p2p_result?;
                Ok(())
            },
            Err(e) => {
              tracing::error!("An error occurred: {:?}", e);
              Err(anyhow::Error::new(e))
            }
        }
    })
}

async fn run_rpc_server(config: Config) -> anyhow::Result<()> {
    tracing::info!("Starting RPC server");
    return Err(anyhow::anyhow!("An error occurred for debugging purposes"));
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
    tracing::info!("RPC server started");
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

async fn run_p2p_server() -> anyhow::Result<()> {
    tracing::info!("Starting P2P server");
    let mut _swarm = libp2p::SwarmBuilder::with_new_identity();

        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;

    tracing::info!("P2P server started");
    Ok(())
}
