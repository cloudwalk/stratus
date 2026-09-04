use std::sync::Arc;

use crate::eth::executor::State;
use crate::eth::executor::types::state::Incomplete;
use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;

pub struct BlockWithChangesFetcher {
    pub chain: Arc<BlockchainClient>,
}

impl DataFetcher for BlockWithChangesFetcher {
    type FetchedType = (BlockRocksdb, BlockChangesRocksdb);
    // If we complete the ExecutionChanges in the fetcher we risk completing with data that is altered by
    // a prior block.
    type PostProcessType = (Block, State<Incomplete>);

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| self.chain.fetch_block_with_changes(bn);
        fetch_with_retry(block_number, fetch_fn, "block and changes").await
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        let (block, changes) = data;
        Ok((block.into(), changes.into()))
    }
}
