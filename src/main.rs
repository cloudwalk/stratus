use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::eth::consensus::simple_consensus::SimpleConsensus;
use stratus::eth::consensus::Consensus;
use stratus::eth::importer::ImporterConfig;
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

    let miner = config.miner.init_with_config(&config, Arc::clone(&storage))?;

    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    let chain = BlockchainClient::init_with_config(&config).await?;

    let consensus: Arc<dyn Consensus> = Arc::new(SimpleConsensus::new(Arc::clone(&storage), chain.clone()));

    let _importer = ImporterConfig::init_with_config(&config, &executor, &miner, &storage, chain.clone())?;

    // init rpc server
    serve_rpc(
        // services
        Arc::clone(&storage),
        executor,
        miner,
        consensus,
        // config
        config.clone(),
        config.rpc_server,
        config.executor.chain_id.into(),
    )
    .await?;

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
