use std::sync::Arc;

use stratus::config::Config;
use stratus::eth::rpc::serve_rpc;
use stratus::init_global_services;

fn main() -> anyhow::Result<()> {
    let config = init_global_services();
    let runtime = config.init_runtime();
    runtime.block_on(run_rpc_server(config))
}

async fn run_rpc_server(config: Config) -> anyhow::Result<()> {
    let storage = config.init_storage().await?;
    let executor = config.init_executor(Arc::clone(&storage));
    serve_rpc(executor, storage, config).await?;
    Ok(())
}
