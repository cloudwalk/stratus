use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;

use crate::GlobalState;
use crate::eth::executor::Changes;
use crate::eth::executor::Executor;
use crate::eth::executor::ExecutorError;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::fetchers::fake_leader::FakeLeaderFetcher;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::interval_miner::commit_retry;
use crate::eth::miner::miner::interval_miner::mine_local_retry;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Block;
use crate::eth::types::StratusError;

pub struct FakeLeaderWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
    pub storage: Arc<StratusStorage>,
}

impl ImportData for <FakeLeaderWorker as ImporterWorker>::DataType {
    fn block_number(&self) -> crate::eth::types::BlockNumber {
        self.0.block_number()
    }
}

#[async_trait]
impl ImporterWorker for FakeLeaderWorker {
    type DataType = <FakeLeaderFetcher as DataFetcher>::PostProcessType;

    async fn import(&self, ((block, _), (expected_block, expected_changes)): Self::DataType) -> anyhow::Result<usize> {
        let block_tx_len = block.transactions.len();
        self.storage.set_pending_from_external(&block);
        for tx in block.0.transactions.into_transactions() {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx.try_into()?) {
                match e {
                    StratusError::Executor(ExecutorError::Nonce { transaction: _, account: _ }) => {
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
        let (mined_block, changes, miner_guard) = mine_local_retry(&self.miner);

        let completed_expected_changes = expected_changes.complete(self.storage.as_ref())?;
        if changes != completed_expected_changes {
            tracing::error!(
                ?changes,
                ?completed_expected_changes,
                "execution changes result mismatch between leader and fake leader"
            );
            bail!("execution changes mismatch between leader and fake leader")
        }

        // `expected_block` is built from `BlockRocksdb` (replicated), which drops per-tx
        // `changes` and `metrics` (replaced with `Default::default()` on the way back). Build a
        // normalized copy of the locally-mined block so the comparison checks fields that actually
        // survive replication, leaving the original untouched for commit.
        let normalized_mined_block = normalize_for_replication_compare(&mined_block);
        if normalized_mined_block != expected_block {
            tracing::error!(?normalized_mined_block, ?expected_block, "block mismatch between leader and fake leader");
            bail!("block mismatch between leader and fake leader")
        }

        commit_retry(&self.miner, mined_block, changes, miner_guard);
        Ok(block_tx_len)
    }
}

/// Builds a copy of `block` with per-tx fields that do not survive `Block -> BlockRocksdb -> Block`
/// replication reset to their defaults, so it can be compared against the leader's replicated
/// block for equivalence. Does not mutate the input.
fn normalize_for_replication_compare(block: &Block) -> Block {
    let mut normalized = block.clone();
    for tx in &mut normalized.transactions {
        tx.execution.result.changes = Changes::default();
    }
    normalized
}
