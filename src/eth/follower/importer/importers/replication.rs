use std::sync::Arc;

use async_trait::async_trait;

use crate::eth::executor::State;
use crate::eth::executor::types::state::Incomplete;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Block;
use crate::infra::kafka::KafkaConnector;

pub struct ReplicationWorker {
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
    pub kafka_connector: Option<KafkaConnector>,
}

impl ImportData for <ReplicationWorker as ImporterWorker>::DataType {
    fn block_number(&self) -> crate::eth::types::BlockNumber {
        self.0.number()
    }
}

#[async_trait]
impl ImporterWorker for ReplicationWorker {
    type DataType = (Block, State<Incomplete>);

    async fn import(&self, (block, changes): Self::DataType) -> anyhow::Result<usize> {
        tracing::info!(block_number = %block.number(), "received block with changes");

        let block_tx_len = block.transactions.len();

        send_block_to_kafka(&self.kafka_connector, &block).await?;

        let completed_changes = changes.complete(self.storage.as_ref())?;
        self.miner.commit(CommitItem::ReplicationBlock(block), completed_changes)?;

        Ok(block_tx_len)
    }
}
