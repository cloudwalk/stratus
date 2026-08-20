use anyhow::bail;
use jsonrpsee::core::client::ClientT;
use serde::de::DeserializeOwned;

use super::blockchain_client::BlockchainClient;
use crate::eth::rpc::types::BlockAndReceiptsPageResponse;
use crate::eth::rpc::types::BlockWithChangesPageResponse;
use crate::eth::rpc::types::IMPORTER_PAGE_LIMIT_DEFAULT;
use crate::eth::rpc::types::ImporterPageInfo;
use crate::eth::rpc::types::ImporterPageRequest;
use crate::eth::rpc::types::PageReducer;
use crate::eth::rpc::types::PaginatedPageFetcher;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::types::BlockNumber;
use crate::eth::types::ExternalBlock;
use crate::eth::types::ExternalBlockWithReceipts;
use crate::eth::types::ExternalReceipt;
use crate::ext::to_json_value;
use crate::log_and_err;

const GET_BLOCK_AND_RECEIPTS: &str = "stratus_getBlockAndReceipts";
const GET_BLOCK_WITH_CHANGES: &str = "stratus_getBlockWithChanges";

pub(super) struct ImporterPaginationClient<'a> {
    client: &'a BlockchainClient,
}

impl<'a> ImporterPaginationClient<'a> {
    pub(super) fn new(client: &'a BlockchainClient) -> Self {
        Self { client }
    }

    pub(super) async fn fetch_block_and_receipts(&self, block_number: BlockNumber) -> anyhow::Result<Option<ExternalBlockWithReceipts>> {
        PaginatedPageFetcher::new(BlockAndReceiptsPages::new(block_number))
            .collect(|cursor| self.fetch_page(GET_BLOCK_AND_RECEIPTS, block_number, cursor, "failed to fetch block with receipts"))
            .await
    }

    pub(super) async fn fetch_block_with_changes(&self, block_number: BlockNumber) -> anyhow::Result<Option<(BlockRocksdb, BlockChangesRocksdb)>> {
        PaginatedPageFetcher::new(BlockWithChangesPages::new(block_number))
            .collect(|cursor| self.fetch_page(GET_BLOCK_WITH_CHANGES, block_number, cursor, "failed to fetch block with changes"))
            .await
    }

    async fn fetch_page<T>(&self, method: &str, block_number: BlockNumber, cursor: Option<String>, error_message: &'static str) -> anyhow::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let params = [
            to_json_value(block_number),
            to_json_value(ImporterPageRequest {
                cursor,
                limit: Some(IMPORTER_PAGE_LIMIT_DEFAULT),
            }),
        ];

        match self.client.http.request::<Option<T>, _>(method, params).await {
            Ok(page) => Ok(page),
            Err(e) => log_and_err!(reason = e, error_message),
        }
    }
}

fn validate_progress(page: &ImporterPageInfo, expected_total: &mut Option<usize>, context: &str) -> anyhow::Result<Option<String>> {
    if page.returned == 0 && page.next_cursor.is_some() {
        bail!("paginated {context} returned no items but provided a next cursor");
    }

    match expected_total {
        Some(expected_total) if *expected_total != page.total => {
            bail!("paginated {context} changed total from {expected_total} to {}", page.total);
        }
        Some(_) => {}
        None => *expected_total = Some(page.total),
    }

    Ok(page.next_cursor.clone())
}

struct BlockAndReceiptsPages {
    block_number: BlockNumber,
    block: Option<ExternalBlock>,
    receipts: Vec<ExternalReceipt>,
    expected_total: Option<usize>,
}

impl BlockAndReceiptsPages {
    fn new(block_number: BlockNumber) -> Self {
        Self {
            block_number,
            block: None,
            receipts: Vec::new(),
            expected_total: None,
        }
    }

    fn push_block(&mut self, page_block: ExternalBlock) -> anyhow::Result<()> {
        if page_block.number() != self.block_number {
            bail!(
                "paginated block with receipts returned unexpected block number {} instead of {}",
                page_block.number(),
                self.block_number
            );
        }

        match &mut self.block {
            Some(block) => block.extend_full_transactions_from(page_block),
            None => {
                self.block = Some(page_block);
                Ok(())
            }
        }
    }
}

impl PageReducer<BlockAndReceiptsPageResponse> for BlockAndReceiptsPages {
    type Output = ExternalBlockWithReceipts;
    type NextPage = String;

    fn reduce(&mut self, page: BlockAndReceiptsPageResponse) -> anyhow::Result<Option<String>> {
        let cursor = validate_progress(&page.pagination, &mut self.expected_total, "block with receipts")?;
        let page_block = ExternalBlock::try_from(page.block)?;
        self.push_block(page_block)?;
        self.receipts.extend(page.receipts);
        Ok(cursor)
    }

    fn finish_after_not_found(self) -> anyhow::Result<Option<Self::Output>> {
        if self.block.is_none() {
            Ok(None)
        } else {
            bail!("block disappeared while fetching paginated block with receipts");
        }
    }

    fn finish(self) -> anyhow::Result<Option<Self::Output>> {
        let Some(block) = self.block else {
            return Ok(None);
        };

        let expected_total = self.expected_total.unwrap_or_default();
        let transactions_len = block.full_transactions_len()?;
        if transactions_len != expected_total {
            bail!("paginated block with receipts assembled {transactions_len} transactions but expected {expected_total}");
        }
        if transactions_len != self.receipts.len() {
            bail!(
                "paginated block with receipts assembled {} transactions but {} receipts",
                transactions_len,
                self.receipts.len()
            );
        }

        Ok(Some(ExternalBlockWithReceipts {
            block,
            receipts: self.receipts,
        }))
    }
}

struct BlockWithChangesPages {
    block_number: BlockNumber,
    block: Option<BlockRocksdb>,
    changes: BlockChangesRocksdb,
    expected_total: Option<usize>,
}

impl BlockWithChangesPages {
    fn new(block_number: BlockNumber) -> Self {
        Self {
            block_number,
            block: None,
            changes: BlockChangesRocksdb::default(),
            expected_total: None,
        }
    }

    fn push_block(&mut self, page_block: BlockRocksdb) -> anyhow::Result<()> {
        let page_block_number = BlockNumber::from(page_block.header.number);
        if page_block_number != self.block_number {
            bail!(
                "paginated block with changes returned unexpected block number {page_block_number} instead of {}",
                self.block_number
            );
        }

        match &mut self.block {
            Some(block) => {
                if block.header.hash != page_block.header.hash {
                    bail!("paginated block with changes changed block hash");
                }
                block.transactions.extend(page_block.transactions);
            }
            None => self.block = Some(page_block),
        }

        Ok(())
    }

    fn push_changes(&mut self, page_changes: BlockChangesRocksdb) -> anyhow::Result<()> {
        for (address, change) in page_changes.account_changes {
            if self.changes.account_changes.insert(address, change).is_some() {
                bail!("paginated block with changes returned duplicate account change");
            }
        }
        for (slot, value) in page_changes.slot_changes {
            if self.changes.slot_changes.insert(slot, value).is_some() {
                bail!("paginated block with changes returned duplicate slot change");
            }
        }
        Ok(())
    }
}

impl PageReducer<BlockWithChangesPageResponse> for BlockWithChangesPages {
    type Output = (BlockRocksdb, BlockChangesRocksdb);
    type NextPage = String;

    fn reduce(&mut self, page: BlockWithChangesPageResponse) -> anyhow::Result<Option<String>> {
        let cursor = validate_progress(&page.pagination, &mut self.expected_total, "block with changes")?;
        self.push_block(page.block)?;
        self.push_changes(page.changes)?;
        Ok(cursor)
    }

    fn finish_after_not_found(self) -> anyhow::Result<Option<Self::Output>> {
        if self.block.is_none() {
            Ok(None)
        } else {
            bail!("block disappeared while fetching paginated block with changes");
        }
    }

    fn finish(self) -> anyhow::Result<Option<Self::Output>> {
        let Some(block) = self.block else {
            return Ok(None);
        };

        let expected_total = self.expected_total.unwrap_or_default();
        let total = block.transactions.len() + self.changes.account_changes.len() + self.changes.slot_changes.len();
        if total != expected_total {
            bail!("paginated block with changes assembled {total} items but expected {expected_total}");
        }

        Ok(Some((block, self.changes)))
    }
}
