use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::config::StratusMode;
use stratus::eth::consensus::simple_consensus::SimpleConsensus;
use stratus::eth::consensus::Consensus;
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

    // init consensus
    let consensus: Arc<dyn Consensus> = Arc::new(SimpleConsensus::new(Arc::clone(&storage), chain.clone()));

    // start importer
    if let StratusMode::Follower = config.mode {
        config
            .importer
            .init(Arc::clone(&executor), Arc::clone(&miner), Arc::clone(&storage), chain.unwrap())?;
        // fix unwrap
    }

    // start rpc server
    let rpc_server_config = config.rpc_server.clone();
    let executor_chain_id = config.executor.chain_id.into();
    let config_clone = config.clone();

    serve_rpc(
        // services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        // config
        config_clone,
        rpc_server_config,
        executor_chain_id,
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
