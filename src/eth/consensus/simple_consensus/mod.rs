use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;

use super::Consensus;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::storage::StratusStorage;
use crate::infra::metrics;
use crate::infra::BlockchainClient;
use crate::log_and_err;

pub struct SimpleConsensus {
    storage: Arc<StratusStorage>,
    // if blockchain_client.is_some() then this is a replica, else this is the validator
    blockchain_client: Option<Arc<BlockchainClient>>,
}

impl SimpleConsensus {
    pub fn new(storage: Arc<StratusStorage>, blockchain_client: Option<Arc<BlockchainClient>>) -> Self {
        SimpleConsensus { storage, blockchain_client }
    }
}

#[async_trait]
impl Consensus for SimpleConsensus {
    fn should_forward(&self) -> bool {
        self.blockchain_client.is_some()
    }
    async fn should_serve(&self) -> bool {
        let Some(blockchain_client) = &self.blockchain_client else {
            return true;
        };

        //gather the latest block number, check how far behind it is from current storage block
        //if its greater than 3 blocks of distance, it should not be served
        let Ok(validator_block_number) = blockchain_client.fetch_block_number().await else {
            tracing::error!("unable to fetch latest block number");
            return false;
        };

        let Ok(current_block_number) = self.storage.read_mined_block_number() else {
            tracing::error!("unable to fetch current block number");
            return false;
        };
        let should_serve = (validator_block_number.as_u64() - 3) <= current_block_number.as_u64();

        if !should_serve {
            tracing::info!(?validator_block_number, ?current_block_number, "validator and replica are too far appart");
        }

        return should_serve;
    }

    async fn forward(&self, transaction: Bytes) -> anyhow::Result<Hash> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let Some(blockchain_client) = &self.blockchain_client else {
            return log_and_err!("SimpleConsensus.forward was called but no blockchain client is set");
        };

        let result = blockchain_client.send_raw_transaction(transaction.into()).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_consensus_forward(start.elapsed());

        let tx_hash = result.tx_hash;
        let validator_url = &blockchain_client.http_url;
        tracing::info!(%tx_hash, ?validator_url, "forwarded eth_sendRawTransaction to leader");

        Ok(result.tx_hash)
    }
}
