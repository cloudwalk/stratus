use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::eth::follower::importer::receive_with_timeout;
use crate::eth::follower::importer::should_shutdown;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::tracing::warn_task_tx_closed;

pub mod execution;
pub mod fake_leader;
pub mod replication;

#[async_trait]
pub trait ImporterWorker<DataType: Send + 'static>: Send + Sync + Sized {
    async fn import(&self, data: DataType) -> anyhow::Result<()>;
    async fn run(self, mut backlog_rx: mpsc::Receiver<DataType>) -> anyhow::Result<()> {
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

            self.import(data).await?;
        }

        warn_task_tx_closed(TASK_NAME);
        Ok(())
    }
}
