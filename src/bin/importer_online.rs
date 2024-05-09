use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use futures::future::try_join_all;

use tokio::task::JoinHandle;

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
use stratus::eth::Executor;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::infra::BlockchainClient;
use stratus::log_and_err;
use stratus::GlobalServices;
use tokio::sync::mpsc;
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
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let executor = config.executor.init(Arc::clone(&storage), relayer).await;
    let miner = config.miner.init(Arc::clone(&storage));
    let chain = BlockchainClient::new(&config.external_rpc).await?;

    run_importer_online(executor, miner, storage, chain).await
}

pub async fn run_importer_online(executor: Arc<Executor>, miner: Arc<BlockMiner>, storage: Arc<StratusStorage>, chain: BlockchainClient) -> anyhow::Result<()> {

    // start from last imported block
    let number = storage.read_mined_block_number().await?;
    let (data_tx, mut data_rx) = mpsc::channel(100);

    tokio::spawn(async move {
        prefetch_blocks_and_receipts(number, chain, data_tx).await;
    });

    while let Some((block, receipts)) = data_rx.recv().await {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        executor.import_external_to_perm(&miner, &block, &receipts).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_import_online(start.elapsed());
    }

    Ok(())
}

async fn prefetch_blocks_and_receipts(mut number: BlockNumber, chain: BlockchainClient, data_tx: mpsc::Sender<(ExternalBlock, ExternalReceipts)>) {
    let mut tasks: HashMap<BlockNumber, JoinHandle<anyhow::Result<(ExternalBlock, ExternalReceipts)>>> = HashMap::new();

    loop {
        let next_number = number.next();
        let chain_clone = chain.clone();
        let task = tokio::spawn(async move {
            let block = fetch_block(&chain_clone, next_number).await?;
            let receipts = fetch_receipts_concurrent(&chain_clone, &block).await?;
            Ok((block, receipts))
        });
        tasks.insert(next_number, task);
        number = next_number;

        if let Some(task) = tasks.remove(&number) {
            if let Ok(Ok((block, receipts))) = task.await {
                data_tx.send((block, receipts)).await.expect("Failed to send block and receipts");
            }
        }
    }
}

async fn fetch_receipts_concurrent(chain: &BlockchainClient, block: &ExternalBlock) -> anyhow::Result<ExternalReceipts> {
    let receipts_futures = block.transactions.iter().map(|tx| {
        fetch_receipt(chain, tx.hash())
    });
    let receipts: Vec<_> = try_join_all(receipts_futures).await?;
    Ok(receipts.into()) // Assuming ExternalReceipts can be constructed from Vec<Receipt>
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
