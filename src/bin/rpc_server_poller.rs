use std::env;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use stratus::eth::primitives::BlockNumber;
use stratus::infra::init_tracing;
use stratus::infra::BlockchainClient;
use tokio::time::sleep;
use tokio::time::timeout;

const POLL_LATENCY: Duration = Duration::from_secs(1);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let Some(rpc_url) = env::args().nth(1) else {
        return Err(anyhow!("rpc url not provided as argument"));
    };

    let chain = Arc::new(BlockchainClient::new(&rpc_url, Duration::from_secs(1))?);

    // TODO instead of gathering the current block all the time, we should track the first block and just keep polling onwards aggregating by 1
    loop {
        tracing::debug!("polling for new blocks");
        let current_block = Arc::clone(&chain).get_current_block_number().await.unwrap();
        tracing::info!("current block number: {}", current_block);
        // TODO call storage in order to save the block details
        let json = block_json(Arc::clone(&chain), current_block).await.unwrap();
        tracing::info!("block: {}", json);
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
