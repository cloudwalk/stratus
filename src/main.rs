use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::Consensus;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init()?;
    let miner = config.miner.init(Arc::clone(&storage))?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));
    let consensus = Consensus::new(
        Arc::clone(&storage),
        Arc::clone(&miner),
        config.storage.perm_storage.rocks_path_prefix.clone(),
        config.clone().candidate_peers.clone(),
        None,
        config.rpc_server.address,
        config.grpc_server_address,
    ); // for now, we force None to initiate with the current node being the leader

    // start rpc server
    serve_rpc(
        // services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        // config
        config.clone(),
        config.rpc_server,
        config.executor.executor_chain_id.into(),
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
