mod importer_online;

use std::sync::Arc;

use importer_online::run_importer_online;
use stratus::config::RunWithImporterConfig;
use stratus::eth::rpc::serve_rpc;
use stratus::init_global_services;
use tokio::try_join;

fn main() -> anyhow::Result<()> {
    let config: RunWithImporterConfig = init_global_services();
    let runtime = config.init_runtime();
    runtime.block_on(run(config))
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    let stratus_config = config.as_stratus();
    let importer_config = config.as_importer();

    #[cfg(feature = "forward_transaction")]
    let (failed_tx_sender, failed_tx_receiver) = tokio::sync::mpsc::channel(1000);

    let storage = stratus_config.stratus_storage.init().await?;

    #[allow(unused_mut)]
    let mut executor = stratus_config.executor.init(Arc::clone(&storage));

    #[cfg(feature = "forward_transaction")]
    executor.set_failed_tx_sender(failed_tx_sender);

    let rpc_task = tokio::spawn(serve_rpc(executor, Arc::clone(&storage), stratus_config));
    let importer_task = tokio::spawn(run_importer_online(
        importer_config,
        storage,
        #[cfg(feature = "forward_transaction")]
        Some(failed_tx_receiver),
    ));

    let join_result = try_join!(rpc_task, importer_task)?;
    join_result.0?;
    join_result.1?;

    Ok(())
}
