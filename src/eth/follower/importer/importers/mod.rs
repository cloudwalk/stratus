use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::eth::follower::importer::receive_with_timeout;
use crate::eth::follower::importer::record_import_metrics;
use crate::eth::follower::importer::should_shutdown;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;

pub mod execution;
pub mod fake_leader;
pub mod replication;

#[async_trait]
pub trait ImporterWorker: Send + Sync + Sized {
    type DataType: Send + 'static;

    /// Import the block. Returns the transaction count of the block.
    async fn import(&self, data: Self::DataType) -> anyhow::Result<usize>;
    async fn run(self, mut backlog_rx: mpsc::Receiver<Self::DataType>) -> anyhow::Result<()> {
        const TASK_NAME: &str = "importer-worker";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;
        loop {
            if should_shutdown(TASK_NAME) {
                return Ok(());
            }

            let data = match receive_with_timeout(&mut backlog_rx).await {
                Ok(Some(inner)) => inner,
                Ok(None) => continue,
                Err(_) => break,
            };

            let start = metrics::now();
            let block_tx_len = self.import(data).await?;
            record_import_metrics(block_tx_len, start.elapsed());
        }

        warn_task_tx_closed(TASK_NAME);
        Ok(())
    }
}
