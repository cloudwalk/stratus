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

    // init services
    let storage = Arc::new(InMemoryStorage::default());
    let evm = Box::new(Revm::new(storage.clone()));
    let executor = EthExecutor::new(evm, storage.clone(), storage.clone());

    serve_rpc(executor, storage.clone(), storage).await?;
    Ok(())
}
