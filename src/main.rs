use std::sync::Arc;

use clap::Parser;
use ledger::config::Config;
use ledger::config::StorageConfig;
use ledger::eth::evm::revm::Revm;
use ledger::eth::rpc::serve_rpc;
use ledger::eth::storage::inmemory::InMemoryStorage;
use ledger::eth::storage::EthStorage;
use ledger::eth::EthExecutor;
use ledger::infra;
use ledger::infra::postgres::Postgres;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // parse cli configs
    let config = Config::parse();

    // init infra
    infra::init_tracing();
    infra::init_metrics();

    // init services
    let storage: Arc<dyn EthStorage> = match config.storage {
        StorageConfig::InMemory => Arc::new(InMemoryStorage::default()),
        StorageConfig::Postgres { url } => Arc::new(Postgres::new(&url).await?),
    };
    let evm = Box::new(Revm::new(Arc::clone(&storage)));
    let executor = EthExecutor::new(evm, Arc::clone(&storage));

    // start rpc server
    serve_rpc(executor, storage).await?;

    Ok(())
}
