use std::sync::Arc;

use crate::eth::follower::importer::BlockchainClient;
use crate::eth::follower::importer::fetch_with_retry;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;

pub struct BlockWithReceiptsFetcher {
    pub chain: Arc<BlockchainClient>,
}

impl DataFetcher for BlockWithReceiptsFetcher {
    type FetchedType = Block;
    type PostProcessType = Block;

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        let fetch_fn = |bn| {
            let chain = Arc::clone(&self.chain);
            async move { chain.fetch_block_and_receipts(bn).await }
        };

        fetch_with_retry(block_number, fetch_fn, "block and receipts").await
    }

    async fn post_process(&self, block: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        for window in block.transactions.windows(2) {
            let tx_index = window[0].mined_data.index.0;
            let next_tx_index = window[1].mined_data.index.0;
            if tx_index + 1 != next_tx_index {
                tracing::error!(tx_index, next_tx_index, "two consecutive transactions must have consecutive indices");
            }
        }

        Ok(block)
    }
}
