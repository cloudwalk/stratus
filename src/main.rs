use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::GlobalServices;
use stratus::GlobalState;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    GlobalState::initialize_node_mode(&global_services.config);
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // Init services
    let storage = config.storage.init()?;

    // Init miner
    let miner = config.miner.init(Arc::clone(&storage))?;

    // Init executor
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // Init importer
    let consensus = if let Some(importer_config) = &config.importer {
        importer_config.init(Arc::clone(&executor), Arc::clone(&miner), Arc::clone(&storage)).await?
    } else {
        None
    };

    // Init RPC server
    serve_rpc(
        // Services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        // Config
        config.clone(),
        config.rpc_server,
        config.executor.executor_chain_id.into(),
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
