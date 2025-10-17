use std::sync::Arc;

use async_trait::async_trait;

use crate::eth::follower::importer::create_execution_changes;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::infra::BlockchainClient;

pub struct BlockWithChangesFetcher {
    pub chain: Arc<BlockchainClient>,
    pub storage: Arc<StratusStorage>,
}

#[async_trait]
impl DataFetcher for BlockWithChangesFetcher {
    type FetchedType = (Block, BlockChangesRocksdb);
    type PostProcessType = (Block, ExecutionChanges);

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| self.chain.fetch_block_with_changes(bn);
        fetch_with_retry(block_number, fetch_fn, "block and changes").await
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        let storage = Arc::clone(&self.storage);
        let (block, changes) = data;
        let changes = create_execution_changes(&storage, changes)?;
        Ok((block, changes))
    }
}
