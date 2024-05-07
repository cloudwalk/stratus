mod importer_online;

use std::sync::Arc;

use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::rpc::serve_rpc;
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


    let stratus_config = config.as_stratus();
    let storage = stratus_config.stratus_storage.init().await?;

    let rpc_task_executor = stratus_config.executor.init(Arc::clone(&storage)).await;
    let rpc_task = serve_rpc(Arc::clone(&rpc_task_executor), Arc::clone(&storage), stratus_config);

    let importer_config = config.as_importer();
    let importer_task_executor = importer_config.executor.init(Arc::clone(&storage)).await;
    let importer_task = run_importer_online(importer_config, Arc::clone(&importer_task_executor), storage);

    try_join!(rpc_task, importer_task)?;
    debug!("rpc and importer tasks finished");

    Ok(())
}
