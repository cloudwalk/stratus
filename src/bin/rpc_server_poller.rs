use std::env;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use clap::Parser;
use hex_literal::hex;
use stratus::config::Config;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::Hash;
use stratus::eth::EthExecutor;
use stratus::infra::init_tracing;
use stratus::infra::BlockchainClient;
use tokio::time::sleep;
use tokio::time::timeout;

const POLL_LATENCY: Duration = Duration::from_secs(1);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config = Config::parse();

    let rpc_url = config.external_rpc.clone().unwrap();
    let chain = Arc::new(BlockchainClient::new(&rpc_url, Duration::from_secs(1))?);

    // Initialize storage and executor for importing the block and receipts.
    let storage = config.init_storage().await?;
    let evms = config.init_evms(Arc::clone(&storage));
    let executor = EthExecutor::new(evms, Arc::clone(&storage));

    let mut current_block_number: BlockNumber = 0.into();
    // TODO instead of gathering the current block all the time, we should track the first block and just keep polling onwards aggregating by 1
    loop {
        let start_time = std::time::Instant::now();

        tracing::debug!("polling for new blocks");
        tracing::info!("current block number: {}", current_block_number);

        // Fetch and parse the block JSON data.
        let block_json = block_json(Arc::clone(&chain), current_block_number).await?;
        tracing::info!("block: {}", block_json);
        let ethers_core_block = stratus::eth::sync_parser::parse_block(&block_json)?;
        // Initialize a vector to store the JSON strings of receipts.
        let mut receipt_jsons: Vec<String> = Vec::new();

        // Fetch the receipt for each transaction in the block.
        for transaction in &ethers_core_block.transactions {
            let receipt = Arc::clone(&chain)
                .get_transaction_receipt(&Hash::from(transaction.hash))
                .await?;
            let receipt_json = serde_json::to_string(&receipt)?;
            receipt_jsons.push(receipt_json);
        }

        // Parse all receipts JSON strings into a HashMap<H256, TransactionReceipt>.
        let receipt_json_strs: Vec<&str> = receipt_jsons.iter().map(AsRef::as_ref).collect();
        let ethers_core_receipts = stratus::eth::sync_parser::parse_receipt(receipt_json_strs)?;

        let fetching_duration = start_time.elapsed();
        let transactions_count = ethers_core_block.transactions.len();
        let receipts_count = receipt_jsons.len(); // Assuming one receipt per transaction

        tracing::info!(
            "Fetching {} receipts for {} transactions in block {} took {:?}",
            receipts_count, transactions_count, current_block_number, fetching_duration
        );

        // Import the block and its associated receipts.
        executor.import(ethers_core_block, ethers_core_receipts).await?;
        current_block_number = current_block_number.next();
        if current_block_number >= Arc::clone(&chain).get_current_block_number().await? {
            tracing::info!("waiting for block number: {}", current_block_number);
            tokio::time::sleep(POLL_LATENCY).await;
        }
    }
}

async fn block_json(chain: Arc<BlockchainClient>, current: BlockNumber) -> anyhow::Result<String> {
    // keep trying to download until success
    //TODO add a backoff limit, not to give up, but to raise latency in hope of network recovery
    loop {
        match timeout(POLL_LATENCY, chain.get_block_by_number(current)).await {
            Ok(Ok(block)) => {
                match serde_json::to_string(&block) {
                    Ok(json) => return Ok(json),
                    Err(e) => {
                        tracing::error!("failed to serialize block to json");
                        return Err(e).context("failed to serialize block to json");
                    }
                };
            }
            // just retry because it is timeout
            Err(e) => {
                tracing::warn!(reason = ?e, "retrying block download");
            }
            // just retry because it is network error
            Ok(Err(e)) => {
                tracing::warn!(reason = ?e, "retrying block download");
            }
        }
    }
}
