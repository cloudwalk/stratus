use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init()?;
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage), None).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer, None).await;

    // start rpc server
    serve_rpc(storage, executor, miner, config.address, config.executor.chain_id.into()).await?;

    Ok(())
}
