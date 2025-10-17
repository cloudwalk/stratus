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
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::BlockchainClient;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_rx_closed;
use crate::log_and_err;

pub struct BlockSaverWorker {
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

#[async_trait]
impl ImporterWorker<(Block, ExecutionChanges)> for BlockSaverWorker {
    async fn import(&self, (block, changes): (Block, ExecutionChanges)) -> anyhow::Result<()> {
        tracing::info!(block_number = %block.number(), "received block with changes");

        #[cfg(feature = "metrics")]
        let (start, block_tx_len) = (metrics::now(), block.transactions.len());

        send_block_to_kafka(&self.kafka_connector, &block).await?;

        self.miner.commit(CommitItem::ReplicationBlock(block), changes)?;

        #[cfg(feature = "metrics")]
        record_import_metrics(block_tx_len, start);

        Ok(())
    }
}
