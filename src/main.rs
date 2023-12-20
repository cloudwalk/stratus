use std::sync::Arc;

use ledger::eth::evm::revm::Revm;
use ledger::eth::rpc::serve_rpc;
use ledger::eth::storage::inmemory::InMemoryStorage;
use ledger::eth::EthExecutor;
use ledger::infra;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // init infra
    infra::init_tracing();
    infra::init_metrics();

    // init services
    let storage = Arc::new(InMemoryStorage::default());
    let evm = Box::new(Revm::new(Arc::clone(&storage)));
    let executor = EthExecutor::new(evm, Arc::clone(&storage), Arc::clone(&storage));

    serve_rpc(executor, Arc::clone(&storage), storage).await?;
    Ok(())
}
