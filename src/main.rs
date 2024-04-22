use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::init_global_services;

fn main() -> anyhow::Result<()> {
    let config: StratusConfig = init_global_services();
    let runtime = config.init_runtime();
    runtime.block_on(run(config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    let storage = config.stratus_storage.init().await?;

    let executor = config.executor.init(Arc::clone(&storage)).await;
    serve_rpc(executor, storage, config).await?;
    Ok(())
}
