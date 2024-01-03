use std::sync::Arc;

use ledger::eth::evm::revm::Revm;
use ledger::eth::rpc::serve_rpc;
use ledger::eth::storage::impls::inmemory::InMemoryStorage;
use ledger::eth::storage::impls::redis::RedisStorage;
use ledger::eth::storage::EthStorage;
use ledger::eth::EthExecutor;
use ledger::infra;

use clap::Parser;
use ledger::config::{Config, Storage};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // init infra
    infra::init_tracing();
    infra::init_metrics();

    let cfg = Config::parse();

    // init services
    let eth_storage: Arc<dyn EthStorage> = match cfg.storage {
        Storage::InMemory => Arc::new(InMemoryStorage::default()),
        Storage::Redis => {
            let url = cfg.redis_url;
            let redis = Arc::new(RedisStorage::new(&url).await?);
            let eth_storage = Arc::clone(&redis) as Arc<dyn EthStorage>;
            eth_storage
        }
    };

    let evm = Box::new(Revm::new(Arc::clone(&eth_storage)));
    let executor = EthExecutor::new(evm, Arc::clone(&eth_storage));

    serve_rpc(executor, eth_storage).await?;
    Ok(())
}
