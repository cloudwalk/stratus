use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::config::StratusMode;
use stratus::eth::consensus::raft::Raft;
use stratus::eth::rpc::serve_rpc;
use stratus::infra::BlockchainClient;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init()?;

    let miner = match config.mode {
        StratusMode::Leader => config.miner.init(Arc::clone(&storage))?,
        StratusMode::Follower => config.miner.init_external_mode(Arc::clone(&storage))?,
    };

    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // init consensus
    let consensus = Some(Raft::new(
        Arc::clone(&storage),
        Arc::clone(&miner),
        config.storage.perm_storage.rocks_path_prefix.clone(),
        config.clone().candidate_peers.clone(),
        None,
        config.rpc_server.address,
        config.grpc_server_address,
    )); // for now, we force None to initiate with the current node being the leader

    // init chain
    let chain = match config.mode {
        StratusMode::Follower => Some(Arc::new(
            BlockchainClient::new_http_ws(
                config.importer.external_rpc.as_deref(),
                config.importer.external_rpc_ws.as_deref(),
                config.importer.external_rpc_timeout,
            )
            .await?,
        )),
        _ => None,
    };

    // start rpc server
    if let Some(consensus) = consensus {
        let rpc_server_config = config.rpc_server.clone();
        let executor_chain_id = config.executor.chain_id.into();
        let config_clone = config.clone();

        serve_rpc(
            // services
            Arc::clone(&storage),
            Arc::clone(&executor),
            Arc::clone(&miner),
            consensus,
            // config
            config_clone,
            rpc_server_config,
            executor_chain_id,
        )
        .await?;
    } else {
        return Err(anyhow::anyhow!("consensus error")); // TODO: better error handling
    }

    // start importer
    if let StratusMode::Follower = config.mode {
        config.importer.init(executor, miner, Arc::clone(&storage), chain.unwrap())?;
        // fix unwrap
    }

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
