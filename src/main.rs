use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init_global_services();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    let storage = config.stratus_storage.init().await?;

    let executor = config.executor.init(Arc::clone(&storage)).await;
    serve_rpc(executor, storage, config).await?;
    Ok(())
}
