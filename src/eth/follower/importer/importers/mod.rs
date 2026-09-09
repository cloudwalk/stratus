use std::future::Future;

use tokio::sync::mpsc;

use crate::GlobalState;
use crate::eth::follower::importer::receive_with_timeout;
use crate::eth::follower::importer::record_import_metrics;
use crate::eth::follower::importer::should_shutdown;
use crate::eth::types::BlockNumber;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::tracing::warn_task_tx_closed;

pub mod blockchain_client;
pub mod execution;
pub mod fake_leader;
pub mod replication;

pub use blockchain_client::BlockchainClient;

pub trait ImportData {
    fn block_number(&self) -> BlockNumber;
}

pub trait ImporterWorker: Send + Sync + Sized {
    type DataType: ImportData + Send + 'static;

    /// Import the block. Returns the transaction count of the block.
    fn import(&self, data: Self::DataType) -> impl Future<Output = anyhow::Result<usize>> + Send;

    fn run(self, mut backlog_rx: mpsc::Receiver<Self::DataType>, stop_at_block: Option<BlockNumber>) -> impl Future<Output = anyhow::Result<()>> + Send {
        async move {
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

                if let Some(target_block) = stop_at_block
                    && data.block_number() > target_block
                {
                    GlobalState::shutdown_importer_from(TASK_NAME, "Importer reached target block");
                    return Ok(());
                }

                let block_tx_len = self.import(data).await?;
                record_import_metrics(block_tx_len);
            }

            warn_task_tx_closed(TASK_NAME);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use tokio::sync::mpsc;

    use crate::GlobalState;
    use crate::eth::follower::importer::importers::ImportData;
    use crate::eth::follower::importer::importers::ImporterWorker;
    use crate::eth::types::BlockNumber;

    /// Minimal block-like datum: just carries a block number.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct NumberedBlock(BlockNumber);

    impl ImportData for NumberedBlock {
        fn block_number(&self) -> BlockNumber {
            self.0
        }
    }

    /// Worker that records every block number it imports.
    struct RecordingWorker {
        imported: Arc<Mutex<Vec<BlockNumber>>>,
    }

    impl ImporterWorker for RecordingWorker {
        type DataType = NumberedBlock;

        async fn import(&self, data: Self::DataType) -> anyhow::Result<usize> {
            self.imported.lock().expect("imported lock").push(data.0);
            Ok(0)
        }
    }

    /// `IMPORTER_SHUTDOWN` defaults to `true`, which makes `should_shutdown()` short-circuit `run`
    /// before it imports anything. The production startup path flips it to `false` before starting
    /// the importer (`ImporterConfig::init_follower_importer`); the test must do the same. This
    /// guard restores the previous value on drop so the test cannot leak state into siblings.
    struct ImporterShutdownGuard(bool);
    impl ImporterShutdownGuard {
        fn new() -> Self {
            let prev = GlobalState::is_importer_shutdown();
            GlobalState::set_importer_shutdown(false);
            Self(prev)
        }
    }
    impl Drop for ImporterShutdownGuard {
        fn drop(&mut self) {
            GlobalState::set_importer_shutdown(self.0);
        }
    }

    /// With `stop_at_block = Some(N)`, the importer must import blocks `1..=N` and stop (returning
    /// `Ok`) when it receives the first block past `N`, without importing it.
    ///
    /// The imported-blocks list is the discriminating assertion: if the stop logic never fired the
    /// worker would also import block 4 (`[1, 2, 3, 4]`); if it fired one block too early (e.g. `>=`
    /// instead of `>`) the worker would stop at `[1, 2]`.
    #[tokio::test]
    async fn importer_stops_at_target_block() {
        let _guard = ImporterShutdownGuard::new();

        let imported = Arc::new(Mutex::new(Vec::new()));
        let worker = RecordingWorker {
            imported: Arc::clone(&imported),
        };

        let (tx, rx) = mpsc::channel::<NumberedBlock>(8);
        for n in [1u64, 2, 3, 4] {
            tx.send(NumberedBlock(BlockNumber::from(n))).await.expect("send block");
        }
        drop(tx);

        let result = worker.run(rx, Some(BlockNumber::from(3u64))).await;
        assert!(result.is_ok(), "importer should stop cleanly at the target block: {result:?}");

        let imported = imported.lock().expect("imported lock");
        assert_eq!(
            *imported,
            vec![BlockNumber::from(1u64), BlockNumber::from(2u64), BlockNumber::from(3u64)],
            "importer must import blocks up to and including the target, but not past it"
        );
    }
}
