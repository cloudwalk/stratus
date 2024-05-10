mod importer_online;

use std::sync::Arc;

use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::infra::BlockchainClient;
use stratus::utils::signal_handler;
use stratus::GlobalServices;
use tokio::try_join;
use tracing::debug;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RunWithImporterConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    //XXX #[cfg(feature = "rocks")]
    //XXX stratus::eth::storage::rocks::consensus::gather_clients().await.unwrap();
    // init services
    let storage = config.stratus_storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage));
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer).await;
    let chain = BlockchainClient::new(&config.external_rpc).await?;
    let cancellation = signal_handler();

    // run rpc and importer-online in parallel
    let rpc_task = serve_rpc(
        Arc::clone(&storage),
        Arc::clone(&executor),
        Arc::clone(&miner),
        config.address,
        config.executor.chain_id.into(),
        cancellation.clone(),
    );
    let importer_task = run_importer_online(executor, miner, storage, chain, cancellation);

    // await both services to finish
    try_join!(rpc_task, importer_task)?;
    debug!("rpc and importer tasks finished");

    Ok(())
}
