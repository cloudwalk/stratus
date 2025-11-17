use std::sync::Arc;

use strum::AsRefStr;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::StratusError;
use crate::eth::rpc::RpcClientApp;
use crate::infra::BlockchainClient;
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
    async fn forward_to_leader(&self, tx_hash: Hash, tx_data: Bytes, rpc_client: &RpcClientApp) -> Result<Hash, StratusError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::info!(%tx_hash, %rpc_client, "forwarding transaction to leader");

        let hash = self.get_chain()?.send_raw_transaction_to_leader(tx_data.into(), rpc_client).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        Ok(hash)
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>>;

    /// Get the lag status between this node and the leader.
    async fn lag(&self) -> anyhow::Result<LagStatus>;
}
