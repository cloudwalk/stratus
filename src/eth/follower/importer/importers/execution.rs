use std::sync::Arc;

use stratus_metrics::timed;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::fetchers::block_with_receipts::BlockOrigin;
use crate::eth::follower::importer::fetchers::block_with_receipts::ImportedBlock;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::importers::reexecute_imported_transactions;
use crate::eth::follower::importer::send_block_to_kafka;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
use crate::ext::to_json_string;
use crate::infra::kafka::KafkaConnector;
use crate::log_and_err;

pub struct ReexecutionWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
    pub kafka_connector: Option<KafkaConnector>,
}

impl ImportData for <ReexecutionWorker as ImporterWorker>::DataType {
    fn block_number(&self) -> BlockNumber {
        self.block_number()
    }
}

impl ImporterWorker for ReexecutionWorker {
    type DataType = ImportedBlock;

    #[timed(import_online_mined_block)]
    async fn import(&self, block: ImportedBlock) -> anyhow::Result<usize> {
        const TASK_NAME: &str = "block-executor";

        let receipts_len = block.receipts_len();

        tracing::info!(block_number = %block.block.number(), "reexecuting imported block");

        for result in reexecute_imported_transactions(self.storage.as_ref(), self.executor.as_ref(), &block.block) {
            if let Err(e) = result {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute imported block");
                return log_and_err!(reason = e, message);
            }
        }

        let (mined_block, changes) = match self.miner.mine_imported(block.block.clone()) {
            Ok((mined_block, changes)) => {
                tracing::info!(number = %mined_block.number(), "mined imported block");
                (mined_block, changes)
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine imported block");
                return log_and_err!(reason = e, message);
            }
        };

        if let Err(e) = compare_with_imported(&mined_block, &block) {
            let message = GlobalState::shutdown_from(TASK_NAME, "imported block mismatch between leader and follower");
            return log_and_err!(reason = e, message);
        }

        send_block_to_kafka(&self.kafka_connector, &mined_block).await?;

        match self.miner.commit(CommitItem::Block(mined_block), changes) {
            Ok(_) => {
                tracing::info!("committed imported block");
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to commit imported block");
                return log_and_err!(reason = e, message);
            }
        }

        Ok(receipts_len)
    }
}

/// Compares the re-executed block against the execution results imported from the leader.
fn compare_with_imported(mined_block: &Block, imported: &ImportedBlock) -> anyhow::Result<()> {
    let block_number = mined_block.number();

    for (mined_tx, imported_tx) in mined_block.transactions.iter().zip(&imported.block.transactions) {
        let mined_result = &mined_tx.execution.output;
        let imported_result = &imported_tx.execution.output;

        let results_match = match imported.origin {
            BlockOrigin::Stratus => mined_result == imported_result,
            // the alloy receipt carries no revert reason and no return output, so those are not comparable
            BlockOrigin::Alloy =>
                mined_result.result.is_success() == imported_result.result.is_success()
                    && mined_result.logs == imported_result.logs
                    && mined_result.gas_used == imported_result.gas_used
                    && mined_result.deployed_contract_address == imported_result.deployed_contract_address,
        };

        if !results_match || mined_tx.mined_data != imported_tx.mined_data {
            let json_mined = to_json_string(mined_result);
            let json_imported = to_json_string(imported_result);
            return log_and_err!(format!(
                "reexecution mismatch between leader and follower | block_number={} tx_hash={} mined={} imported={}",
                block_number, mined_tx.execution.info.hash, json_mined, json_imported
            ));
        }
    }

    Ok(())
}
