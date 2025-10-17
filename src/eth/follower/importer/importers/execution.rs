use std::cmp::min;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::yield_now;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::EXTERNAL_RPC_CURRENT_BLOCK;
use crate::eth::follower::importer::create_execution_changes;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::FetcherWorker;
use crate::eth::follower::importer::importers::ImporterWorker;
#[cfg(feature = "metrics")]
use crate::eth::follower::importer::record_import_metrics;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::follower::importer::should_shutdown;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::BlockchainClient;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_rx_closed;
use crate::log_and_err;

pub struct BlockExecutorWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

#[async_trait]
impl ImporterWorker<(ExternalBlock, Vec<ExternalReceipt>)> for BlockExecutorWorker {
    async fn import(&self, (block, receipts): (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<()> {
        const TASK_NAME: &str = "block-executor";

        #[cfg(feature = "metrics")]
        let (start, block_number, block_tx_len, receipts_len) = (metrics::now(), block.number(), block.transactions.len(), receipts.len());

        if let Err(e) = self.executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts)) {
            let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute external block");
            return log_and_err!(reason = e, message);
        };

        // statistics
        #[cfg(feature = "metrics")]
        {
            use crate::ext::DisplayExt;
            use crate::utils::calculate_tps;

            let duration = start.elapsed();
            let tps = calculate_tps(duration, block_tx_len);

            tracing::info!(
                tps,
                %block_number,
                duration = %duration.to_string_ext(),
                %receipts_len,
                "reexecuted external block",
            );
        }

        let (mined_block, changes) = match self.miner.mine_external(block) {
            Ok((mined_block, changes)) => {
                tracing::info!(number = %mined_block.number(), "mined external block");
                (mined_block, changes)
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
                return log_and_err!(reason = e, message);
            }
        };

        send_block_to_kafka(&self.kafka_connector, &mined_block).await?;

        match self.miner.commit(CommitItem::Block(mined_block), changes) {
            Ok(_) => {
                tracing::info!("committed external block");
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to commit external block");
                return log_and_err!(reason = e, message);
            }
        }

        #[cfg(feature = "metrics")]
        record_import_metrics(receipts_len, start);

        Ok(())
    }
}
