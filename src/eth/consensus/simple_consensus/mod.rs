use std::sync::Arc;

use async_trait::async_trait;

use super::Consensus;
use crate::eth::storage::StratusStorage;
use crate::infra::BlockchainClient;

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
    async fn lag(&self) -> anyhow::Result<u64> {
        let Some(blockchain_client) = &self.blockchain_client else {
            return Err(anyhow::anyhow!("blockchain client not set"));
        };

        let validator_block_number = blockchain_client.fetch_block_number().await?;
        let current_block_number = self.storage.read_mined_block_number()?;

        Ok(validator_block_number.as_u64() - current_block_number.as_u64())
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>> {
        let Some(chain) = &self.blockchain_client else {
            return Err(anyhow::anyhow!("blockchain client not set"));
        };
        Ok(chain)
    }
}
