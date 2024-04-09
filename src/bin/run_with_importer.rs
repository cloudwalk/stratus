use std::sync::Arc;

use futures::StreamExt;
use futures::TryStreamExt;
use stratus::config::ImporterOnlineConfig;
use stratus::config::RunWithImporterConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::Hash;
use stratus::eth::rpc::serve_rpc;
use stratus::eth::storage::StratusStorage;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use stratus::log_and_err;
use tokio::try_join;

const RECEIPTS_PARALELLISM: usize = 10;

fn main() -> anyhow::Result<()> {
    let config: RunWithImporterConfig = init_global_services();
    let runtime = config.init_runtime();
    runtime.block_on(run(config))
}

async fn run(config: RunWithImporterConfig) -> anyhow::Result<()> {
    let stratus_config = config.as_stratus();
    let importer_config = config.as_importer();

    let storage = stratus_config.stratus_storage.init().await?;
    let executor = stratus_config.executor.init(Arc::clone(&storage));

    let rpc_task = tokio::spawn(serve_rpc(executor, Arc::clone(&storage), stratus_config));
    let importer_task = tokio::spawn(run_importer(importer_config, storage));

    let join_result = try_join!(rpc_task, importer_task)?;
    join_result.0?;
    join_result.1?;

    Ok(())
}

// TODO: I extracted this from importer_online.rs, in the future it'd make sense to extract this function into a separate file.
async fn run_importer(config: ImporterOnlineConfig, storage: Arc<StratusStorage>) -> anyhow::Result<()> {
    // init services
    let chain = BlockchainClient::new(&config.external_rpc).await?;
    let executor = config.executor.init(Arc::clone(&storage));

    // start from last imported block
    let mut number = storage.read_mined_block_number().await?;

    // keep importing forever
    loop {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        number = number.next();

        // fetch block and receipts
        let block = fetch_block(&chain, number).await?;

        // fetch receipts in parallel
        let mut receipts = Vec::with_capacity(block.transactions.len());
        for tx in &block.transactions {
            receipts.push(fetch_receipt(&chain, tx.hash()));
        }
        let receipts = futures::stream::iter(receipts).buffered(RECEIPTS_PARALELLISM).try_collect::<Vec<_>>().await?;

        // import block
        let receipts: ExternalReceipts = receipts.into();
        executor.import_external_to_perm(block, &receipts).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_import_online(start.elapsed());
    }
}

async fn fetch_block(chain: &BlockchainClient, number: BlockNumber) -> anyhow::Result<ExternalBlock> {
    let block = loop {
        tracing::info!(%number, "fetching block");
        let block = match chain.get_block_by_number(number).await {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(reason = ?e, "retrying block download because error");
                continue;
            }
        };

        if block.is_null() {
            #[cfg(not(feature = "perf"))]
            {
                tracing::warn!(reason = %"null", "retrying block download because block is not mined yet");
                continue;
            }

            #[cfg(feature = "perf")]
            std::process::exit(0);
        }

        break block;
    };

    match serde_json::from_value(block.clone()) {
        Ok(block) => Ok(block),
        Err(e) => log_and_err!(reason = e, payload = block, "failed to deserialize external block"),
    }
}

async fn fetch_receipt(chain: &BlockchainClient, hash: Hash) -> anyhow::Result<ExternalReceipt> {
    let receipt = loop {
        tracing::info!(%hash, "fetching receipt");
        let receipt = match chain.get_transaction_receipt(&hash).await {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(reason = ?e, "retrying receipt download because error");
                continue;
            }
        };

        if receipt.is_null() {
            tracing::warn!(reason = %"null", "retrying receipt download because block is not mined yet");
            continue;
        }

        break receipt;
    };

    match serde_json::from_value(receipt.clone()) {
        Ok(receipt) => Ok(receipt),
        Err(e) => log_and_err!(reason = e, payload = receipt, "failed to deserialize external receipt"),
    }
}
