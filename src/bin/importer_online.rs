use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

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
use stratus::utils::signal_handler;
use stratus::GlobalServices;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[allow(dead_code)]
fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOnlineConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOnlineConfig) -> anyhow::Result<()> {
    let storage = config.stratus_storage.init().await?;
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage));
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer).await;
    let chain = BlockchainClient::new(&config.external_rpc).await?;
    let cancellation: CancellationToken = signal_handler();

    let result = run_importer_online(executor, miner, storage, chain, cancellation, config.sync_interval).await;
    if let Err(ref e) = result {
        tracing::error!(reason = ?e, "importer-online failed");
    }
    result
}

pub async fn run_importer_online(
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    storage: Arc<StratusStorage>,
    chain: BlockchainClient,
    cancellation: CancellationToken,
    sync_interval: u64,
) -> anyhow::Result<()> {
    // start from last imported block
    let mut number = storage.read_mined_block_number().await?;
    let (data_tx, mut data_rx) = mpsc::channel(10);

    if number != BlockNumber::from(0) {
        number = number.next();
    }

    let task_cancellation = cancellation.clone();
    tokio::spawn(async move {
        prefetch_blocks_and_receipts(number, chain, data_tx, task_cancellation, sync_interval).await;
    });

    while let Some((block, receipts)) = data_rx.recv().await {
        if cancellation.is_cancelled() {
            tracing::info!("run_importer_online task cancelled, exiting");
            break;
        }

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        executor.reexecute_external(&block, &receipts).await?;

        // mine block
        miner.mine_external_mixed_and_commit().await?;

        #[cfg(feature = "metrics")]
        metrics::inc_n_importer_online_transactions_total(receipts.len() as u64);
        #[cfg(feature = "metrics")]
        metrics::inc_import_online_mined_block(start.elapsed());
    }

    Ok(())
}

async fn prefetch_blocks_and_receipts(
    mut number: BlockNumber,
    chain: BlockchainClient,
    data_tx: mpsc::Sender<(ExternalBlock, ExternalReceipts)>,
    cancellation: CancellationToken,
    sync_interval: u64,
) {
    let buffered_data = Arc::new(RwLock::new(HashMap::new()));
    let chain_clone = chain.clone();

    // This task will handle the ordered sending of blocks and receipts
    {
        let buffered_data = Arc::clone(&buffered_data);
        tokio::spawn(async move {
            let mut next_block_number = number;
            loop {
                if cancellation.is_cancelled() {
                    tracing::info!("prefetch_blocks_and_receipts task cancelled, closing channel");
                    break;
                }
                let mut data = buffered_data.write().await;

                //if it is close to the last block, use the sync interval
                match chain_clone.get_current_block_number().await {
                    Ok(current_block_number) =>
                        if current_block_number < next_block_number.next() {
                            sleep(Duration::from_millis(sync_interval)).await;
                        },
                    Err(e) => {
                        tracing::error!("failed to get current block number {:?}", e);
                        sleep(Duration::from_millis(sync_interval)).await;
                    }
                }

                if let Some((block, receipts)) = data.remove(&next_block_number) {
                    data_tx.send((block, receipts)).await.expect("Failed to send block and receipts");
                    next_block_number = next_block_number.next();
                }
            }
        });
    }

    loop {
        let mut handles = Vec::new();
        // Spawn tasks for concurrent fetching
        for _ in 0..2 {
            // Number of concurrent fetch tasks
            let chain = chain.clone();
            let buffered_data = Arc::clone(&buffered_data);
            handles.push(tokio::spawn(async move {
                let block = fetch_block(&chain, number).await.unwrap();
                let receipts = fetch_receipts_in_parallel(&chain, &block).await;

                let mut data = buffered_data.write().await;
                data.insert(number, (block, receipts.clone().into()));
            }));

            number = number.next();
        }

        futures::future::join_all(handles).await;
    }
}

// This is an example helper function to fetch receipts in parallel
async fn fetch_receipts_in_parallel(chain: &BlockchainClient, block: &ExternalBlock) -> Vec<ExternalReceipt> {
    let receipts_futures = block.transactions.iter().map(|tx| fetch_receipt(chain, tx.hash()));
    futures::future::join_all(receipts_futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
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
