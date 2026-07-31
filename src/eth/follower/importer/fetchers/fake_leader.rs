use async_trait::async_trait;

use super::block_with_changes::BlockWithChangesFetcher;
use super::block_with_receipts::BlockWithReceiptsFetcher;
use crate::eth::follower::importer::fetchers::DataFetcher;
use crate::eth::primitives::BlockNumber;

pub struct FakeLeaderFetcher {
    pub block_with_receipts_fetcher: BlockWithReceiptsFetcher,
    pub block_with_changes_fetcher: BlockWithChangesFetcher,
}

#[async_trait]
impl DataFetcher for FakeLeaderFetcher {
    type FetchedType = (
        <BlockWithReceiptsFetcher as DataFetcher>::FetchedType,
        <BlockWithChangesFetcher as DataFetcher>::FetchedType,
    );
    type PostProcessType = (
        <BlockWithReceiptsFetcher as DataFetcher>::PostProcessType,
        <BlockWithChangesFetcher as DataFetcher>::PostProcessType,
    );

    async fn fetch(&self, block_number: BlockNumber) -> Self::FetchedType {
        (
            self.block_with_receipts_fetcher.fetch(block_number).await,
            self.block_with_changes_fetcher.fetch(block_number).await,
        )
    }

    async fn post_process(&self, data: Self::FetchedType) -> anyhow::Result<Self::PostProcessType> {
        Ok((
            self.block_with_receipts_fetcher.post_process(data.0).await?,
            self.block_with_changes_fetcher.post_process(data.1).await?,
        ))
    }
}
