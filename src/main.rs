use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::utils::signal_handler;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // init cancellation handler
    let cancellation = signal_handler();

    // init services
    let storage = config.storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let external_relayer = if let Some(c) = config.external_relayer { Some(c.init().await) } else { None };
    let miner = config.miner.init(Arc::clone(&storage), None, external_relayer, cancellation.clone()).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer, None).await;

    // start rpc server
    serve_rpc(storage, executor, miner, config.address, config.executor.chain_id.into(), cancellation).await?;

    Ok(())
}
