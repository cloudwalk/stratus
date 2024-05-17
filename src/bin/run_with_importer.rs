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

/// TODO: There are platform-independent functions to get the hostname.
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

// XXX this is a temporary solution to get the leader node
// later we want the leader to GENERATE blocks
// and even later we want this sync to be replaced by a gossip protocol or raft
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
    config.online.external_rpc
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    // init services
    let storage = config.storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage)).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer).await;
    let http_url = get_chain_url(config.clone());
    let chain = Arc::new(BlockchainClient::new_http(&http_url).await?);
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
