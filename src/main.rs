use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::follower::consensus::Consensus;
use stratus::eth::rpc::serve_rpc;
use stratus::infra::BlockchainClient;
use stratus::GlobalServices;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<StratusConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: StratusConfig) -> anyhow::Result<()> {
    // Init services
    let storage = config.storage.init()?;

    // Init miner
    let miner = if config.follower {
        config.miner.init_external_mode(Arc::clone(&storage))?
    } else {
        config.miner.init(Arc::clone(&storage))?
    };

    // Init executor
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // Init importer
    let consensus: Option<Arc<dyn Consensus>> = if config.follower {
        let importer_config = config.importer.as_ref().ok_or(anyhow::anyhow!("importer config is not set"))?;
        let chain = Arc::new(
            BlockchainClient::new_http_ws(
                importer_config.external_rpc.as_ref(),
                importer_config.external_rpc_ws.as_deref(),
                importer_config.external_rpc_timeout,
            )
            .await?,
        );
        Some(importer_config.init(Arc::clone(&executor), Arc::clone(&miner), Arc::clone(&storage), Arc::clone(&chain))?)
    } else {
        None
    };

    // Init RPC server
    serve_rpc(
        // Services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        // Config
        config.clone(),
        config.rpc_server,
        config.executor.executor_chain_id.into(),
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
