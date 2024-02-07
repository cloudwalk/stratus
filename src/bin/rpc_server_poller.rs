use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use hex_literal::hex;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::Hash;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use tokio::time::sleep;
use tokio::time::timeout;

const POLL_LATENCY: Duration = Duration::from_secs(1);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let config = init_global_services();

    let rpc_url = config.external_rpc.clone().unwrap();
    let chain = Arc::new(BlockchainClient::new(&rpc_url, Duration::from_secs(1))?);

    // TODO instead of gathering the current block all the time, we should track the first block and just keep polling onwards aggregating by 1
    loop {
        tracing::debug!("polling for new blocks");
        let current_block = Arc::clone(&chain).get_current_block_number().await?;
        tracing::info!("current block number: {}", current_block);
        // TODO call storage in order to save the block details
        let block_json = block_json(Arc::clone(&chain), current_block).await?;
        tracing::info!("block: {}", block_json);
        let ethers_core_block = stratus::eth::sync_parser::parse_block(&block_json)?;

        let receipt = Arc::clone(&chain)
            .get_transaction_receipt(&Hash::new(hex!("5a493d5b9e4f36a7569407988e2f9506334de3a7ce3b451c594ab1496cc5e13f")))
            .await?;
        let receipt_json = serde_json::to_string(&receipt)?;
        tracing::info!("receipt_json: {:?}", receipt_json);
        let ethers_core_receipts = stratus::eth::sync_parser::parse_receipt(vec![&receipt_json])?;

        let storage = config.init_storage().await?;
        let executor = config.init_executor(Arc::clone(&storage));
        executor.import(ethers_core_block, ethers_core_receipts).await?;

        sleep(POLL_LATENCY).await;
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
