use std::sync::Arc;

use stratus::config::StratusConfig;
use stratus::config::StratusMode;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::Consensus;
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
        StratusMode::Stratus => config.miner.init(Arc::clone(&storage))?,
        StratusMode::ImporterOnline | StratusMode::RunWithImporter => config.miner.init_external_mode(Arc::clone(&storage))?,
    };

    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // init consensus
    let consensus = match config.mode {
        StratusMode::Stratus | StratusMode::RunWithImporter => Some(Consensus::new(
            Arc::clone(&storage),
            Arc::clone(&miner),
            config.storage.perm_storage.rocks_path_prefix.clone(),
            config.clone().candidate_peers.clone(),
            None,
            config.rpc_server.address,
            config.grpc_server_address,
        )),
        _ => None,
    }; // for now, we force None to initiate with the current node being the leader

    // init chain
    let chain = match config.mode {
        StratusMode::ImporterOnline => Some(Arc::new(
            BlockchainClient::new_http_ws(
                config.importer.external_rpc,
                config.importer.external_rpc_ws.as_deref(),
                config.importer.external_rpc_timeout,
            )
            .await?,
        )),
        StratusMode::RunWithImporter => {
            let (http_url, ws_url) = consensus.get_chain_url().await.expect("chain url not found");

            Some(Arc::new(
                BlockchainClient::new_http_ws(&http_url, ws_url.as_deref(), config.importer.external_rpc_timeout).await?,
            ))
        }
        _ => None,
    };

    // start rpc server
    match config.mode {
        StratusMode::Stratus | StratusMode::RunWithImporter => {
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
        }
        _ => {}
    }

    // start importer
    match config.mode {
        StratusMode::ImporterOnline | StratusMode::RunWithImporter => {
            config.importer.init(executor, miner, Arc::clone(&storage), chain.unwrap())?;
        }
        _ => {}
    }

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}
