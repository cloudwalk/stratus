use std::sync::Arc;

use async_trait::async_trait;

use crate::eth::follower::importer::importers::ImporterWorker;
#[cfg(feature = "metrics")]
use crate::eth::follower::importer::record_import_metrics;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::primitives::Block;
use crate::eth::primitives::ExecutionChanges;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

pub struct ReplicationWorker {
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

#[async_trait]
impl ImporterWorker for ReplicationWorker {
    type DataType = (Block, ExecutionChanges);

    async fn import(&self, (block, changes): Self::DataType) -> anyhow::Result<()> {
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
