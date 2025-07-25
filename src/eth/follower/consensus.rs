use std::sync::Arc;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::StratusError;
use crate::eth::rpc::RpcClientApp;
use crate::infra::BlockchainClient;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

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

        let should_serve = lag <= 3;

        if !should_serve {
            tracing::warn!(?lag, "validator and replica are too far apart");
        }

        should_serve
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

    /// Get the lag between this node and the leader.
    async fn lag(&self) -> anyhow::Result<u64>;
}
