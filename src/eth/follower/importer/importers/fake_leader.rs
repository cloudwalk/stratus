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
use crate::eth::miner::miner::interval_miner::mine_and_commit;
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

pub struct FakeLeaderWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
}

#[async_trait]
impl ImporterWorker<(ExternalBlock, Vec<ExternalReceipt>)> for FakeLeaderWorker {
    async fn import(&self, (block, _): (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<()> {
        for tx in block.0.transactions.into_transactions() {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx.try_into()?) {
                match e {
                    StratusError::Transaction(TransactionError::Nonce { transaction: _, account: _ }) => {
                        tracing::warn!(reason = ?e, "transaction failed, was this node restarted?");
                    }
                    _ => {
                        tracing::error!(reason = ?e, "transaction failed");
                        GlobalState::shutdown_from("Importer (FakeMiner)", "Transaction Failed");
                        bail!(e);
                    }
                }
            }
        }
        mine_and_commit(&self.miner);
        Ok(())
    }
}
