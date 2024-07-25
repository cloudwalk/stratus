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
                config.importer.external_rpc.as_deref(),
                config.importer.external_rpc_ws.as_deref(),
                config.importer.external_rpc_timeout,
            )
            .await?,
        )),
        StratusMode::RunWithImporter =>
            if let Some(consensus) = &consensus {
                let (http_url, ws_url) = consensus.get_chain_url().await?;
                Some(Arc::new(
                    BlockchainClient::new_http_ws(Some(&http_url), ws_url.as_deref(), config.importer.external_rpc_timeout).await?,
                ))
            } else {
                return Err(anyhow::anyhow!("cannot get chain url")); // TODO: better error handling
            },
        _ => None,
    };

    // start rpc server
    if let Some(consensus) = consensus {
        let rpc_server_config = config.rpc_server.clone();
        let executor_chain_id = config.executor.chain_id.into();
        let config_clone = config.clone();

        serve_rpc(
            // services
            Arc::clone(&storage),
            Arc::clone(&executor),
            Arc::clone(&miner),
            consensus,
            // config
            config_clone,
            rpc_server_config,
            executor_chain_id,
        )
        .await?;
    } else {
        return Err(anyhow::anyhow!("consensus error")); // TODO: better error handling
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
