use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::importer::importers::ImporterWorker;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::interval_miner::mine_and_commit;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;

pub struct FakeLeaderWorker {
    pub executor: Arc<Executor>,
    pub miner: Arc<Miner>,
}

#[async_trait]
impl ImporterWorker for FakeLeaderWorker {
    type DataType = (ExternalBlock, Vec<ExternalReceipt>);

    async fn import(&self, (block, _): Self::DataType) -> anyhow::Result<()> {
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
