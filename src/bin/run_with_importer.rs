mod importer_online;

use std::sync::Arc;

use anyhow::anyhow;
use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::Consensus;
use stratus::infra::BlockchainClient;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::join;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RunWithImporterConfig>::init()?;
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    const TASK_NAME: &str = "run-with-importer";

    // init services
    let storage = config.storage.init().await?;
    let consensus = Consensus::new(Arc::clone(&storage), Some(config.clone())).await; // in development, with no leader configured, the current node ends up being the leader
    let Some((http_url, ws_url)) = consensus.get_chain_url() else {
        return Err(anyhow!("No chain url found"));
    };
    let chain = Arc::new(BlockchainClient::new_http_ws(&http_url, ws_url.as_deref(), config.online.external_rpc_timeout).await?);

    let relayer = config.relayer.init().await?;
    let external_relayer = if let Some(c) = config.external_relayer { Some(c.init().await) } else { None };
    let miner = config
        .miner
        .init_external_mode(Arc::clone(&storage), Some(Arc::clone(&consensus)), external_relayer)
        .await?;
    let executor = config
        .executor
        .init(Arc::clone(&storage), Arc::clone(&miner), relayer, Some(Arc::clone(&consensus)))
        .await;

    let rpc_storage = Arc::clone(&storage);
    let rpc_executor = Arc::clone(&executor);
    let rpc_miner = Arc::clone(&miner);

    // run rpc and importer-online in parallel
    let rpc_task = async move {
        let res = serve_rpc(
            rpc_storage,
            rpc_executor,
            rpc_miner,
            Arc::clone(&consensus),
            config.address,
            config.executor.chain_id.into(),
        )
        .await;
        GlobalState::shutdown_from(TASK_NAME, "rpc server finished unexpectedly");
        res
    };

    let importer_task = async move {
        let res = run_importer_online(executor, miner, storage, chain, config.online.sync_interval).await;
        GlobalState::shutdown_from(TASK_NAME, "importer online finished unexpectedly");
        res
    };

    // await both services to finish
    let (rpc_result, importer_result) = join!(rpc_task, importer_task);
    tracing::debug!(?rpc_result, ?importer_result, "rpc and importer tasks finished");
    rpc_result?;
    importer_result?;

    Ok(())
}
