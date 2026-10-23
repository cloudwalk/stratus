use std::sync::Arc;

use stratus_metrics::timed;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::fetchers::block_with_receipts::FetchedBlockWithReceipts;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::types::ExternalReceipts;
use crate::infra::kafka::KafkaConnector;
use crate::log_and_err;

pub struct ReexecutionWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub kafka_connector: Option<KafkaConnector>,
}

impl ImportData for <ReexecutionWorker as ImporterWorker>::DataType {
    fn block_number(&self) -> crate::eth::types::BlockNumber {
        self.block_number()
    }
}

impl ImporterWorker for ReexecutionWorker {
    type DataType = FetchedBlockWithReceipts;

    #[timed(import_online_mined_block)]
    async fn import(&self, block: Self::DataType) -> anyhow::Result<usize> {
        const TASK_NAME: &str = "block-executor";

        let receipts_len = block.receipts_len();

        let (mined_block, changes) = match block {
            FetchedBlockWithReceipts::Alloy { block, receipts } => {
                if let Err(e) = self.executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts)) {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute external block");
                    return log_and_err!(reason = e, message);
                };

                match self.miner.mine_external(block) {
                    Ok((mined_block, changes)) => {
                        tracing::info!(number = %mined_block.number(), "mined external block");
                        (mined_block, changes)
                    }
                    Err(e) => {
                        let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
                        return log_and_err!(reason = e, message);
                    }
                }
            }
            FetchedBlockWithReceipts::Stratus(block) => {
                if let Err(e) = self.executor.execute_imported_block(block.clone()) {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute imported block");
                    return log_and_err!(reason = e, message);
                };

                match self.miner.mine_imported(block) {
                    Ok((mined_block, changes)) => {
                        tracing::info!(number = %mined_block.number(), "mined imported block");
                        (mined_block, changes)
                    }
                    Err(e) => {
                        let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine imported block");
                        return log_and_err!(reason = e, message);
                    }
                }
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
