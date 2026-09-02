use std::sync::Arc;

use strum::AsRefStr;

use crate::eth::executor::AccessListOutput;
use crate::eth::executor::Executor;
use crate::eth::rpc::BlockchainClient;
use crate::eth::types::Bytes;
use crate::eth::types::ExecutionKind;
use crate::eth::types::Hash;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

const MAX_ALLOWED_LAG_BLOCKS: u64 = 3;

#[derive(Clone, Copy, Debug, AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum LagDirection {
    Ahead,
    Behind,
}

#[derive(Clone, Copy, Debug)]
pub struct LagStatus {
    pub distance: u64,
    pub direction: LagDirection,
}

impl LagStatus {
    pub fn is_ahead(&self) -> bool {
        matches!(self.direction, LagDirection::Ahead)
    }

    pub fn is_far_behind(&self) -> bool {
        matches!(self.direction, LagDirection::Behind) && self.distance > MAX_ALLOWED_LAG_BLOCKS
    }
}

#[allow(async_fn_in_trait)]
pub trait Consensus: Send + Sync {
    /// Whether this node should serve requests.
    async fn should_serve(&self) -> bool {
        let lag = match self.lag().await {
            Ok(lag) => lag,
            Err(err) => {
                tracing::error!(?err, "failed to get the lag between this node and the leader");
                return false;
            }
        };

        if lag.is_far_behind() {
            tracing::warn!(blocks_behind = lag.distance, "validator and replica are too far apart");
        }

        if lag.is_ahead() {
            tracing::warn!(distance = lag.distance, "follower is ahead of the leader");
        }

        !(lag.is_far_behind() || lag.is_ahead())
    }

    /// Forwards a transaction to leader.
    ///
    /// The current machine name is sent as the `x-client` header by `BlockchainClient`, so the leader
    /// attributes the transaction to this node automatically.
    async fn forward_to_leader(&self, tx: TransactionInput, tx_hash: Hash, tx_data: Bytes) -> Result<Hash, StratusError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::info!(%tx_hash, "forwarding transaction to leader");

        let access_list = self // make this configurable (?)
            .get_executor()
            .execute_local_call::<AccessListOutput>(tx.into(), ExecutionKind::AccessList)?;

        let hash = self.get_client().send_raw_transaction_to_leader(tx_data.into(), Some(access_list)).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        Ok(hash)
    }

    fn get_client(&self) -> &Arc<BlockchainClient>;

    fn get_executor(&self) -> &Arc<Executor>;

    /// Get the lag status between this node and the leader.
    async fn lag(&self) -> anyhow::Result<LagStatus>;
}
