use std::sync::Arc;

use clap::Parser;
use stratus::config::Config;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::EthExecutor;
use stratus::infra;

fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    // init global services
    infra::init_tracing();
    infra::init_metrics();
    let runtime = config.init_runtime();

    // start rpc server
    runtime.block_on(run_rpc_server(config))
}

async fn run_rpc_server(config: Config) -> anyhow::Result<()> {
    let storage = config.init_storage().await?;
    let evms = config.init_evms(Arc::clone(&storage));
    let executor = EthExecutor::new(evms, Arc::clone(&storage));

    serve_rpc(executor, storage, config).await?;
    Ok(())
}
