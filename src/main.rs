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
    let storage = config.storage.init().await?;
    let external_relayer = if let Some(c) = config.clone().external_relayer {
        Some(c.init().await)
    } else {
        None
    };
    let miner = config.miner.init(Arc::clone(&storage), None, external_relayer).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner)).await;
    let consensus = Consensus::new(
        Arc::clone(&storage),
        config.clone().candidate_peers.clone(),
        None,
        config.address,
        config.grpc_server_address,
    )
    .await; // for now, we force None to initiate with the current node being the leader

    // start rpc server
    serve_rpc(
        storage,
        executor,
        miner,
        consensus,
        config.address,
        config.executor.chain_id.into(),
        config.max_connections,
    )
    .await?;

    Ok(())
}
