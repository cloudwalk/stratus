mod importer_online;

use std::sync::Arc;

use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::consensus::Consensus;
use stratus::eth::rpc::serve_rpc;
use stratus::infra::BlockchainClient;
use stratus::utils::signal_handler;
use stratus::GlobalServices;
use tokio::join;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RunWithImporterConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init().await?;
    let consensus = Arc::new(Consensus::new(config.clone().leader_node)); // in development, with no leader configured, the current node ends up being the leader
    let (http_url, ws_url) = consensus.get_chain_url(config.clone());
    consensus.sender.send("Consensus initialized.".to_string()).await.unwrap();
    let chain = Arc::new(BlockchainClient::new_http_ws(&http_url, ws_url.as_deref()).await?);

    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage)).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer, Some(consensus)).await;

    let rpc_storage = Arc::clone(&storage);
    let rpc_executor = Arc::clone(&executor);
    let rpc_miner = Arc::clone(&miner);

    let cancellation = signal_handler();
    let rpc_cancellation = cancellation.clone();

    // run rpc and importer-online in parallel
    let rpc_task = async move {
        let res = serve_rpc(
            rpc_storage,
            rpc_executor,
            rpc_miner,
            config.address,
            config.executor.chain_id.into(),
            rpc_cancellation.clone(),
        )
        .await;
        tracing::warn!("serve_rpc finished, cancelling tasks");
        rpc_cancellation.cancel();
        res
    };

    let importer_task = async move {
        let res = run_importer_online(executor, miner, storage, chain, cancellation.clone(), config.online.sync_interval).await;
        tracing::warn!("run_importer_online finished, cancelling tasks");
        cancellation.cancel();
        res
    };

    // await both services to finish
    let (rpc_result, importer_result) = join!(rpc_task, importer_task);
    tracing::debug!(?rpc_result, ?importer_result, "rpc and importer tasks finished");
    rpc_result?;
    importer_result?;

    Ok(())
}
