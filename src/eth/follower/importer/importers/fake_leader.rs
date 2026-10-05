use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::bail;
use anyhow::ensure;
use stratus_metrics::timed;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::executor::ExecutorError;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::fetchers::fake_leader::FakeLeaderFetcher;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::follower::importer::importers::fake_leader_comparison::ReplayPrestate;
use crate::eth::follower::importer::importers::fake_leader_comparison::compare_blocks;
use crate::eth::follower::importer::importers::fake_leader_comparison::compare_state_changes;
use crate::eth::follower::importer::importers::fake_leader_comparison::validate_original_values;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::interval_miner::commit_retry;
use crate::eth::storage::StratusStorage;
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

impl ImporterWorker for FakeLeaderWorker {
    type DataType = <FakeLeaderFetcher as DataFetcher>::PostProcessType;

    #[timed(import_online_mined_block)]
    async fn import(&self, ((block, _), (expected_block, expected_changes)): Self::DataType) -> anyhow::Result<usize> {
        let block_tx_len = block.transactions.len();
        self.storage.set_pending_from_external(&block);
        for tx in block.0.transactions.into_transactions() {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx.try_into()?, None) {
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

        let miner_guard = self.miner.locks.mine_and_commit.lock();
        let (mined_block, changes) = self.miner.mine_local();
        ensure!(
            mined_block.number().prev() == Some(self.storage.read_mined_block_number()),
            "fake leader comparison requires the preceding block as permanent state for block {}",
            mined_block.number()
        );

        let addresses = changes
            .accounts
            .keys()
            .chain(expected_changes.accounts.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let slot_keys = changes.slots.keys().chain(expected_changes.slots.keys()).copied().collect::<BTreeSet<_>>();
        let prestate = ReplayPrestate {
            accounts: self.storage.perm.read_accounts(addresses.into_iter().collect())?.into_iter().collect(),
            slots: self.storage.perm.read_slots(slot_keys.into_iter().collect())?.into_iter().collect(),
        };
        let read_account = |address| Ok(prestate.account(address));
        let read_slot = |address, index| Ok(prestate.slot(address, index));
        let final_expected_changes = expected_changes.complete(&prestate)?.finalize();
        let final_changes = changes.clone().finalize();
        validate_original_values(&changes, &read_account, &read_slot)
            .and_then(|()| compare_state_changes(&final_changes, &final_expected_changes, read_account, read_slot))
            .inspect_err(|error| {
                tracing::error!(block_number = %mined_block.number(), reason = %error, "execution changes result mismatch between leader and fake leader");
            })?;

        compare_blocks(&mined_block, &expected_block).inspect_err(|error| {
            tracing::error!(block_number = %mined_block.number(), reason = %error, "block mismatch between leader and fake leader");
        })?;

        commit_retry(&self.miner, mined_block, changes, miner_guard);
        Ok(block_tx_len)
    }
}
