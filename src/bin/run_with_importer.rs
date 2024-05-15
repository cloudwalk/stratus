mod importer_online;

use std::env;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::infra::BlockchainClient;
use stratus::utils::signal_handler;
use stratus::GlobalServices;
use tokio::join;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RunWithImporterConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

fn current_node() -> Option<String> {
    let mut file = File::open("/etc/hostname").ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    Some(contents.trim().to_string())
}

fn current_namespace() -> Option<String> {
    let namespace = env::var("NAMESPACE").ok()?;
    Some(namespace.trim().to_string())
}

fn get_chain_url(config: RunWithImporterConfig) -> String {
    if let Some(leader_node) = config.leader_node {
        if let Some(current_node) = current_node() {
            if current_node != leader_node {
                if let Some(namespace) = current_namespace() {
                    return format!("http://{}.stratus-api.{}.svc.cluster.local:3000", leader_node, namespace);
                }
            }
        }
    }
    config.external_rpc
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.stratus_storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage));
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer).await;
    let chain_url = get_chain_url(config.clone());
    let chain = BlockchainClient::new(&chain_url).await?;
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
        let res = run_importer_online(executor, miner, storage, chain, cancellation.clone(), config.sync_interval).await;
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
