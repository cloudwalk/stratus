use std::sync::Arc;

use parking_lot::RwLock;
use stratus::GlobalServices;
use stratus::GlobalState;
use stratus::config::StratusConfig;
use stratus::eth::rpc::Server;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    GlobalState::initialize_node_mode(&global_services.config);
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // Init services
    let storage = config.storage.init()?;

    // Init miner
    let miner = config.miner.init(Arc::clone(&storage)).await?;

    // Init executor
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    let (consensus, importer_runtime) = if let Some(importer_config) = &config.importer {
        tracing::info!(?importer_config, "creating importer");
        let kafka_connector = config.kafka_config.as_ref().map(|inner| inner.init()).transpose()?;
        match importer_config
            .init(Arc::clone(&executor), Arc::clone(&miner), Arc::clone(&storage), kafka_connector)
            .await?
        {
            Some((consensus, importer_runtime)) => (Some(consensus), Some(importer_runtime)),
            None => (None, None),
        }
    } else {
        tracing::info!("no importer config, skipping importer");
        (None, None)
    };
    let consensus = Arc::new(RwLock::new(consensus));
    let importer_runtime = Arc::new(RwLock::new(importer_runtime));

    // Init RPC server
    Server::new(
        // Services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        importer_runtime,
        // Config
        config.clone(),
        config.rpc_server,
        config.executor.executor_chain_id.into(),
    )
    .serve()
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
