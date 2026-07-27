use std::sync::Arc;

use async_trait::async_trait;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::infra::kafka::KafkaConnector;
use crate::log_and_err;

pub struct ReexecutionWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

#[async_trait]
impl ImporterWorker for ReexecutionWorker {
    type DataType = (ExternalBlock, Vec<ExternalReceipt>);

    async fn import(&self, (block, receipts): Self::DataType) -> anyhow::Result<usize> {
        const TASK_NAME: &str = "block-executor";

        let receipts_len = receipts.len();

        if let Err(e) = self.executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts)) {
            let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute external block");
            return log_and_err!(reason = e, message);
        };

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

        Ok(receipts_len)
    }
}
