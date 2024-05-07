use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::TryStreamExt;
use serde::Deserialize;
use stratus::config::ImporterOnlineConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::Hash;
use stratus::eth::storage::StratusStorage;
use stratus::eth::BlockMiner;
use stratus::eth::EthExecutor;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::infra::BlockchainClient;
use stratus::log_and_err;
use stratus::GlobalServices;
use tokio::time::sleep;

/// Number of transactions receipts that can be fetched in parallel.
const RECEIPTS_PARALELLISM: usize = 10;

#[allow(dead_code)]
fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOnlineConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOnlineConfig) -> anyhow::Result<()> {
    let storage = config.stratus_storage.init().await?;
    let executor = config.executor.init(Arc::clone(&storage)).await;
    let miner = config.miner.init(Arc::clone(&storage));
    let chain = BlockchainClient::new(&config.external_rpc).await?;

    run_importer_online(executor, miner, storage, chain).await
}

pub async fn run_importer_online(
    executor: Arc<EthExecutor>,
    miner: Arc<BlockMiner>,
    storage: Arc<StratusStorage>,
    chain: BlockchainClient,
) -> anyhow::Result<()> {
    // start from last imported block
    let mut number = storage.read_mined_block_number().await?;

    // keep importing forever
    loop {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        number = number.next();
        import(&executor, &miner, &chain, number).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_import_online(start.elapsed());
    }
}

#[tracing::instrument(skip_all)]
async fn import(executor: &EthExecutor, miner: &BlockMiner, chain: &BlockchainClient, number: BlockNumber) -> anyhow::Result<()> {
    // fetch block and receipts
    let block = fetch_block(chain, number).await?;

    #[cfg(feature = "metrics")]
    let start = metrics::now();

    // fetch receipts in parallel
    let mut receipts = Vec::with_capacity(block.transactions.len());
    for tx in &block.transactions {
        receipts.push(fetch_receipt(chain, tx.hash()));
    }
    let receipts = futures::stream::iter(receipts).buffered(RECEIPTS_PARALELLISM).try_collect::<Vec<_>>().await?;

    // import block
    let receipts: ExternalReceipts = receipts.into();
    executor.import_external_to_perm(miner, &block, &receipts).await?;

    #[cfg(feature = "metrics")]
    metrics::inc_n_importer_online_transactions_total(receipts.len() as u64);
    #[cfg(feature = "metrics")]
    metrics::inc_import_online_mined_block(start.elapsed());

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn fetch_block(chain: &BlockchainClient, number: BlockNumber) -> anyhow::Result<ExternalBlock> {
    let mut delay = 10;
    let block = loop {
        tracing::info!(%number, "fetching block");
        let block = match chain.get_block_by_number(number).await {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(reason = ?e, "retrying block download because error");
                sleep(Duration::from_millis(delay)).await;
                delay *= 2;
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

    match ExternalBlock::deserialize(&block) {
        Ok(block) => Ok(block),
        Err(e) => log_and_err!(reason = e, payload = block, "failed to deserialize external block"),
    }
}

#[tracing::instrument(skip_all)]
async fn fetch_receipt(chain: &BlockchainClient, hash: Hash) -> anyhow::Result<ExternalReceipt> {
    let receipt = loop {
        tracing::info!(%hash, "fetching receipt");
        let receipt = chain.get_transaction_receipt(hash).await?;

        match receipt {
            Some(receipt) => break receipt,
            None => {
                tracing::warn!(reason = %"null", "retrying receipt download because block is not mined yet");
                continue;
            }
        }
    };

    Ok(receipt)
}
