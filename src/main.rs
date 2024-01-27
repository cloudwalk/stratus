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
use stratus::infra;
use stratus::infra::postgres::Postgres;
use tokio::sync::broadcast;
use tokio::select;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;


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

fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    infra::init_tracing();
    infra::init_metrics();

    let runtime = init_async_runtime();
    let (tx, _) = broadcast::channel::<()>(1);

    runtime.block_on(async {
        let rpc_handle = tokio::spawn(run_rpc_server(config, tx.subscribe()));
        let p2p_handle = tokio::spawn(run_p2p_server(tx.subscribe()));

        if let Err(e) = rpc_handle.await? {
            tx.send(()).unwrap(); // Send cancellation signal
            return Err(e);
        }

        if let Err(e) = p2p_handle.await? {
            tx.send(()).unwrap(); // Send cancellation signal
            return Err(e);
        }

        Ok(())
    })
}

async fn run_rpc_server(config: Config, mut cancel_signal: broadcast::Receiver<()>) -> anyhow::Result<()> {
    tracing::info!("Starting RPC server");

    let storage: Arc<dyn EthStorage> = match config.storage {
        StorageConfig::InMemory => Arc::new(InMemoryStorage::default().metrified()),
        StorageConfig::Postgres { url } => Arc::new(Postgres::new(&url).await?.metrified()),
    };
    let evms = init_evms(Arc::clone(&storage));
    let executor = EthExecutor::new(evms, Arc::clone(&storage));

    serve_rpc(executor, storage, config.address).await?;

    tracing::info!("RPC server started");
    Ok(())
}

fn init_evms(storage: Arc<dyn EthStorage>) -> NonEmpty<Box<dyn Evm>> {
    let cpu_cores = thread::available_parallelism().unwrap();
    let num_evms = max(1, cpu_cores.get() - 2);

    let mut evms: Vec<Box<dyn Evm>> = Vec::with_capacity(10);
    for _ in 1..=num_evms {
        evms.push(Box::new(Revm::new(Arc::clone(&storage))));
    }
    tracing::info!(evms = %num_evms, "evms initialized");
    NonEmpty::from_vec(evms).unwrap()
}

async fn run_p2p_server(mut cancel_signal: broadcast::Receiver<()>) -> anyhow::Result<()> {
    tracing::info!("Starting P2P server");
    let mut _swarm = libp2p::SwarmBuilder::with_new_identity();

    //XXX return Err(anyhow::anyhow!("An error occurred for debugging purposes"));

    loop {
        select! {
            _ = cancel_signal.recv() => {
                tracing::info!("P2P task cancelled");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(1000)) => {
                tracing::info!("P2P task running");
            }
        }
    }

    tracing::info!("P2P server stopped");
    Ok(())
}
