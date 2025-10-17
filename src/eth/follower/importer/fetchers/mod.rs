use std::cmp::min;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::yield_now;

use crate::eth::follower::importer::EXTERNAL_RPC_CURRENT_BLOCK;
use crate::eth::follower::importer::should_shutdown;
use crate::eth::primitives::BlockNumber;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::tracing::warn_task_rx_closed;

pub mod block_with_changes;
pub mod block_with_receipts;

/// Number of blocks that are downloaded in parallel.
const PARALLEL_BLOCKS: usize = 3;

#[async_trait]
pub trait FetcherWorker<FetchedType: Send + 'static, PostProcessType: Send + 'static>: Send + Sync + Sized {
    async fn fetch(&self, block_number: BlockNumber) -> FetchedType;
    async fn post_process(&self, data: FetchedType) -> anyhow::Result<PostProcessType>;

    /// Generic block fetcher that handles the common logic of fetching blocks in parallel
    async fn run(self, backlog_tx: mpsc::Sender<PostProcessType>, mut importer_block_number: BlockNumber) -> anyhow::Result<()> {
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;
        const TASK_NAME: &str = "importer block fetcher";

        loop {
            if should_shutdown(TASK_NAME) {
                return Ok(());
            }

            // if we are ahead of current block number, await until we are behind again
            let external_rpc_current_block = EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::Relaxed);
            if importer_block_number.as_u64() > external_rpc_current_block {
                yield_now().await;
                continue;
            }

            // we are behind current, so we will fetch multiple blocks in parallel to catch up
            let blocks_behind = importer_block_number.count_to(external_rpc_current_block);
            let mut blocks_to_fetch = min(blocks_behind, 1_000); // avoid spawning millions of tasks (not parallelism), at least until we know it is safe
            tracing::info!(%blocks_behind, blocks_to_fetch, "catching up with blocks");

            let mut tasks = Vec::with_capacity(blocks_to_fetch as usize);
            while blocks_to_fetch > 0 {
                blocks_to_fetch -= 1;
                tasks.push(self.fetch(importer_block_number));
                importer_block_number = importer_block_number.next_block_number();
            }

            // keep fetching in order
            let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
            while let Some(fetched_data) = tasks.next().await {
                let processed = self.post_process(fetched_data).await?;
                if backlog_tx.send(processed).await.is_err() {
                    warn_task_rx_closed(TASK_NAME);
                    return Ok(());
                }
            }
        }
    }
}
