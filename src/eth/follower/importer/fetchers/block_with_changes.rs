use std::sync::Arc;

use async_trait::async_trait;

use crate::eth::executor::ExecutionChanges;
use crate::eth::executor::Incomplete;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::rpc::BlockchainClient;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;

pub struct BlockWithChangesFetcher {
    pub chain: Arc<BlockchainClient>,
}

#[async_trait]
impl DataFetcher for BlockWithChangesFetcher {
    type FetchedType = (BlockRocksdb, BlockChangesRocksdb);
    // If we complete the ExecutionChanges in the fetcher we risk completing with data that is altered by
    // a prior block.
    type PostProcessType = (Block, ExecutionChanges<Incomplete>);

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| self.chain.fetch_block_with_changes(bn);
        fetch_with_retry(block_number, fetch_fn, "block and changes").await
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        let (block, changes) = data;
        Ok((block.into(), changes.into()))
    }
}
