use std::sync::Arc;

use stratus::config::StratusConfig;
#[cfg(feature = "request-replication-test-sender")]
use stratus::eth::rpc::replication_worker;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::Consensus;
#[cfg(feature = "request-replication-test-sender")]
use stratus::ext::spawn_named;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init()?;
    let external_relayer = if let Some(c) = config.clone().external_relayer {
        Some(c.init().await)
    } else {
        None
    };
    let miner = config.miner.init(Arc::clone(&storage), external_relayer)?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));
    let consensus = Consensus::new(
        Arc::clone(&storage),
        Arc::clone(&miner),
        config.storage.perm_storage.rocks_path_prefix.clone(),
        config.clone().candidate_peers.clone(),
        None,
        config.address,
        config.grpc_server_address,
    ); // for now, we force None to initiate with the current node being the leader

    #[cfg(feature = "request-replication-test-sender")]
    let tx = {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spawn_named("replication::sender", replication_worker(config.replicate_request_to, rx));
        tx
    };

    // start rpc server
    serve_rpc(
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        config.address,
        config.executor.chain_id.into(),
        config.max_connections,
        #[cfg(feature = "request-replication-test-sender")]
        tx,
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
