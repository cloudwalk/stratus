use std::sync::Arc;

use clap::Parser;
use stratus::config::Config;
use stratus::config::StorageConfig;
use stratus::eth::evm::revm::Revm;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::storage::EthStorage;
use stratus::eth::storage::InMemoryStorage;
use stratus::eth::EthExecutor;
use stratus::infra;
use stratus::infra::postgres::Postgres;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // parse cli configs
    let config = Config::parse();

    // init infra
    infra::init_tracing();
    infra::init_metrics();

    // init services
    let storage: Arc<dyn EthStorage> = match config.storage {
        StorageConfig::InMemory => Arc::new(InMemoryStorage::default().metrified()),
        StorageConfig::Postgres { url } => Arc::new(Postgres::new(&url).await?.metrified()),
    };
    let evm = Box::new(Revm::new(Arc::clone(&storage)));
    let executor = EthExecutor::new(evm, Arc::clone(&storage));

    // start rpc server
    serve_rpc(executor, storage).await?;

    Ok(())
}
