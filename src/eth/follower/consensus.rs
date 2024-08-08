use std::sync::Arc;

use async_trait::async_trait;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;

#[async_trait]
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
            tracing::info!(?lag, "validator and replica are too far apart");
        }

        return should_serve;
    }

    /// Forward a transaction
    async fn forward(&self, transaction: Bytes) -> anyhow::Result<Hash> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let blockchain_client = self.get_chain()?;

        let result = blockchain_client.send_raw_transaction(transaction.into()).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        let tx_hash = result.tx_hash;
        let validator_url = &blockchain_client.http_url;
        tracing::info!(%tx_hash, ?validator_url, "forwarded eth_sendRawTransaction to leader");

        Ok(result.tx_hash)
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>>;

    /// Get the lag between this node and the leader.
    async fn lag(&self) -> anyhow::Result<u64>;
}
