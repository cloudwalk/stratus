use std::sync::Arc;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::StratusError;
use crate::eth::rpc::RpcClientApp;
use crate::infra::BlockchainClient;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[derive(Clone, Copy, Debug)]
pub struct LagStatus {
    pub blocks_behind: u64,
    pub is_ahead: bool,
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

        let is_far_behind = lag.blocks_behind > 3;
        let is_ahead = lag.is_ahead;

        if is_far_behind {
            tracing::warn!(blocks_behind = lag.blocks_behind, "validator and replica are too far apart");
        }

        if is_ahead {
            tracing::warn!("follower is ahead of the leader");
        }

        !(is_far_behind || is_ahead)
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
