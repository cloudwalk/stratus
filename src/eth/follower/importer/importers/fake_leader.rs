use std::sync::Arc;

use anyhow::bail;
use stratus_metrics::timed;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::executor::ExecutorError;
use crate::eth::executor::types::TransactionExecution;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::follower::importer::fetchers::block_with_receipts::FetchedBlockWithReceipts;
use crate::eth::follower::importer::fetchers::fake_leader::FakeLeaderFetcher;
use crate::eth::follower::importer::importers::ImportData;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::interval_miner::commit_retry;
use crate::eth::storage::StratusStorage;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;

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
    async fn import(&self, (fetched, (expected_block, expected_changes)): Self::DataType) -> anyhow::Result<usize> {
        let (block_tx_len, transactions) = match fetched {
            FetchedBlockWithReceipts::Alloy { block, .. } => {
                let block_tx_len = block.transactions.len();
                self.storage.set_pending_from_external(&block);
                let transactions = block
                    .0
                    .transactions
                    .into_transactions()
                    .map(|tx| tx.try_into())
                    .collect::<Result<Vec<TransactionInput>, _>>()?;
                (block_tx_len, transactions)
            }
            FetchedBlockWithReceipts::Stratus(mut block) => {
                let block_tx_len = block.transactions.len();
                self.storage.set_pending_header(block.number(), block.timestamp());
                let transactions = std::mem::take(&mut block.transactions)
                    .into_iter()
                    .map(|tx| -> anyhow::Result<TransactionInput> {
                        let TransactionExecution {
                            info,
                            signature,
                            input: stored_input,
                            output: _,
                        } = tx.execution;
                        let tx_input = TransactionInput {
                            transaction_info: info,
                            execution_info: stored_input.into(),
                            signature,
                        };
                        tx_input.recover_signer_address()?;
                        Ok(tx_input)
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                (block_tx_len, transactions)
            }
        };

        for tx in transactions {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx, None) {
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

        let final_expected_changes = expected_changes.complete(self.storage.as_ref())?.finalize();
        let final_changes = changes.clone().finalize();
        if final_changes != final_expected_changes {
            tracing::error!(?mined_block, "execution changes result mismatch between leader and fake leader");
            bail!("execution changes mismatch between leader and fake leader")
        }

        if mined_block != expected_block {
            tracing::error!(?mined_block, ?expected_block, "block mismatch between leader and fake leader");
            bail!("block mismatch between leader and fake leader")
        }

        commit_retry(&self.miner, mined_block, changes, miner_guard);
        Ok(block_tx_len)
    }
}
