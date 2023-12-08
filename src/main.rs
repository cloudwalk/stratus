use std::sync::Arc;
use std::sync::Mutex;

use ledger::eth::evm::revm::Revm;
use ledger::eth::rpc::serve_rpc;
use ledger::eth::storage::inmemory::InMemoryStorage;
use ledger::infra;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // init infra
    infra::init_tracing();

    // init services
    let eth_storage = Arc::new(InMemoryStorage::new());
    let evm = Box::new(Mutex::new(Revm::new(eth_storage.clone())));

    serve_rpc(evm, eth_storage).await?;
    Ok(())
}
