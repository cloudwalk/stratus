use anyhow::bail;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClient;
use serde::de::DeserializeOwned;

use crate::eth::rpc::types::BlockAndReceiptsPageResponse;
use crate::eth::rpc::types::BlockWithChangesPageResponse;
use crate::eth::rpc::types::CursorPageInfo;
use crate::eth::rpc::types::ImporterPageRequest;
use crate::eth::rpc::types::ImporterPagination;
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
    http: &'a HttpClient,
}

impl<'a> ImporterPaginationClient<'a> {
    pub(super) fn new(http: &'a HttpClient) -> Self {
        Self { http }
    }

    pub(super) async fn fetch_block_and_receipts(&self, block_number: BlockNumber) -> anyhow::Result<Option<ExternalBlockWithReceipts>> {
        PaginatedPageFetcher::new(BlockAndReceiptsReducer::new(block_number))
            .collect(|cursor| self.fetch_page(GET_BLOCK_AND_RECEIPTS, block_number, cursor, "failed to fetch block with receipts"))
            .await
    }

    pub(super) async fn fetch_block_with_changes(&self, block_number: BlockNumber) -> anyhow::Result<Option<(BlockRocksdb, BlockChangesRocksdb)>> {
        PaginatedPageFetcher::new(BlockWithChangesReducer::new(block_number))
            .collect(|cursor| self.fetch_page(GET_BLOCK_WITH_CHANGES, block_number, cursor, "failed to fetch block with changes"))
            .await
    }

    async fn fetch_page<T>(&self, method: &str, block_number: BlockNumber, cursor: Option<String>, error_message: &'static str) -> anyhow::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        // first request is a plain request: the leader decides whether the response
        // needs pagination (by size). Continuations carry only the returned cursor.
        let params = match cursor {
            Some(cursor) => vec![to_json_value(block_number), to_json_value(ImporterPageRequest::with_cursor(cursor))],
            None => vec![to_json_value(block_number)],
        };

        match self.http.request::<Option<T>, _>(method, params).await {
            Ok(page) => Ok(page),
            Err(e) => log_and_err!(reason = e, error_message),
        }
    }
}

fn validate_page(
    page: &CursorPageInfo,
    expected_total: &mut Option<usize>,
    last_cursor_index: &mut Option<usize>,
    context: &str,
) -> anyhow::Result<Option<String>> {
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

    if let Some(cursor) = &page.next_cursor {
        let (_, next_index) =
            ImporterPagination::decode_cursor_for_validation(cursor).map_err(|e| anyhow::anyhow!("paginated {context} returned an undecodable cursor: {e}"))?;
        if let Some(last) = *last_cursor_index
            && next_index <= last
        {
            bail!("paginated {context} did not advance: cursor index went from {last} to {next_index}");
        }
        *last_cursor_index = Some(next_index);
    }

    Ok(page.next_cursor.clone())
}

struct BlockAndReceiptsReducer {
    block_number: BlockNumber,
    block: Option<ExternalBlock>,
    receipts: Vec<ExternalReceipt>,
    expected_total: Option<usize>,
    last_cursor_index: Option<usize>,
}

impl BlockAndReceiptsReducer {
    fn new(block_number: BlockNumber) -> Self {
        Self {
            block_number,
            block: None,
            receipts: Vec::new(),
            expected_total: None,
            last_cursor_index: None,
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

impl PageReducer<BlockAndReceiptsPageResponse> for BlockAndReceiptsReducer {
    type Output = ExternalBlockWithReceipts;
    type NextPage = String;

    fn reduce(&mut self, page: BlockAndReceiptsPageResponse) -> anyhow::Result<Option<String>> {
        let cursor = match &page.pagination {
            Some(pagination) => validate_page(pagination, &mut self.expected_total, &mut self.last_cursor_index, "block with receipts")?,
            None => None,
        };
        let page_block = page.block;
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

        let transactions_len = block.try_full_transactions_len()?;

        if let Some(expected_total) = self.expected_total
            && transactions_len != expected_total
        {
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

struct BlockWithChangesReducer {
    block_number: BlockNumber,
    block: Option<BlockRocksdb>,
    changes: BlockChangesRocksdb,
    expected_total: Option<usize>,
    last_cursor_index: Option<usize>,
}

impl BlockWithChangesReducer {
    fn new(block_number: BlockNumber) -> Self {
        Self {
            block_number,
            block: None,
            changes: BlockChangesRocksdb::default(),
            expected_total: None,
            last_cursor_index: None,
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

impl PageReducer<BlockWithChangesPageResponse> for BlockWithChangesReducer {
    type Output = (BlockRocksdb, BlockChangesRocksdb);
    type NextPage = String;

    fn reduce(&mut self, page: BlockWithChangesPageResponse) -> anyhow::Result<Option<String>> {
        let cursor = match &page.pagination {
            Some(pagination) => validate_page(pagination, &mut self.expected_total, &mut self.last_cursor_index, "block with changes")?,
            None => None,
        };
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

        if let Some(expected_total) = self.expected_total {
            let total = block.transactions.len() + self.changes.account_changes.len() + self.changes.slot_changes.len();
            if total != expected_total {
                bail!("paginated block with changes assembled {total} items but expected {expected_total}");
            }
        }

        Ok(Some((block, self.changes)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fake::Fake;
    use fake::Faker;
    use hash_hasher::HashBuildHasher;

    use super::BlockAndReceiptsReducer;
    use super::BlockWithChangesReducer;
    use super::validate_page;
    use crate::eth::rpc::types::BlockAndReceiptsPageResponse;
    use crate::eth::rpc::types::BlockWithChangesPageResponse;
    use crate::eth::rpc::types::CursorPageInfo;
    use crate::eth::rpc::types::PageReducer;
    use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::ExternalBlock;
    use crate::eth::types::ExternalReceipt;
    use crate::eth::types::SlotIndex;
    use crate::eth::types::UnixTime;

    // helpers

    fn page_info(returned: usize, total: usize, next_cursor: Option<&str>) -> CursorPageInfo {
        CursorPageInfo {
            limit: 256,
            returned,
            total,
            next_cursor: next_cursor.map(String::from),
        }
    }

    fn external_block_with_txs(count: usize) -> ExternalBlock {
        let mut block: ExternalBlock = Faker.fake();
        let txs: Vec<_> = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        block.0.transactions = alloy_rpc_types_eth::BlockTransactions::Full(txs);
        block
    }

    fn split_external_block(block: &ExternalBlock, at: usize) -> (ExternalBlock, ExternalBlock) {
        let alloy_rpc_types_eth::BlockTransactions::Full(txs) = &block.0.transactions else {
            panic!("test fixture blocks always carry full transactions");
        };
        let txs = txs.clone();
        let mut page1 = block.clone();
        let mut page2 = block.clone();
        page1.0.transactions = alloy_rpc_types_eth::BlockTransactions::Full(txs[..at].to_vec());
        page2.0.transactions = alloy_rpc_types_eth::BlockTransactions::Full(txs[at..].to_vec());
        (page1, page2)
    }

    fn receipts_page(block: ExternalBlock, receipts: Vec<ExternalReceipt>, pagination: CursorPageInfo) -> BlockAndReceiptsPageResponse {
        BlockAndReceiptsPageResponse {
            block,
            receipts,
            pagination: Some(pagination),
        }
    }

    /// Builds a cursor string in the format used by the server, pointing at the given index.
    fn cursor_at(index: usize) -> String {
        format!("v1:0x{}:{index}", "ab".repeat(32))
    }

    fn block_rocksdb_with_txs(number: u64, count: usize) -> BlockRocksdb {
        let mut block = Block::new(BlockNumber::from(number), UnixTime::from(0u64));
        block.transactions = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        BlockRocksdb::from(block)
    }

    fn changes_with_accounts(addresses: &[[u8; 20]]) -> BlockChangesRocksdb {
        let mut account_changes = HashMap::with_hasher(HashBuildHasher::default());
        for addr in addresses {
            account_changes.insert(AddressRocksdb(*addr), AccountChangesRocksdb::default());
        }
        BlockChangesRocksdb {
            account_changes,
            slot_changes: HashMap::with_hasher(HashBuildHasher::default()),
        }
    }

    fn changes_with_slots(slots: &[(AddressRocksdb, SlotIndexRocksdb)]) -> BlockChangesRocksdb {
        let mut slot_changes = HashMap::with_hasher(HashBuildHasher::default());
        for (addr, idx) in slots {
            slot_changes.insert((*addr, *idx), SlotValueRocksdb::default());
        }
        BlockChangesRocksdb {
            account_changes: HashMap::with_hasher(HashBuildHasher::default()),
            slot_changes,
        }
    }

    fn slot_idx(a: u64, b: u64, c: u64, d: u64) -> SlotIndexRocksdb {
        SlotIndexRocksdb::from(SlotIndex::from([a, b, c, d]))
    }

    // validate_page

    #[test]
    fn validate_page_first_page_sets_expected_total() {
        let cursor = cursor_at(3);
        let mut expected_total = None;
        let mut last_index = None;
        let info = page_info(3, 10, Some(&cursor));
        let result = validate_page(&info, &mut expected_total, &mut last_index, "test").expect("ok");
        assert_eq!(expected_total, Some(10));
        assert_eq!(result, Some(cursor));
    }

    #[test]
    fn validate_page_same_total_ok() {
        let cursor = cursor_at(3);
        let mut expected_total = Some(10);
        let mut last_index = None;
        let info = page_info(3, 10, Some(&cursor));
        let result = validate_page(&info, &mut expected_total, &mut last_index, "test").expect("ok");
        assert_eq!(result, Some(cursor));
    }

    #[test]
    fn validate_page_different_total_errors() {
        let cursor = cursor_at(3);
        let mut expected_total = Some(10);
        let mut last_index = None;
        let info = page_info(3, 20, Some(&cursor));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_err());
    }

    #[test]
    fn validate_page_zero_returned_with_cursor_errors() {
        let cursor = cursor_at(0);
        let mut expected_total = None;
        let mut last_index = None;
        let info = page_info(0, 10, Some(&cursor));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_err());
    }

    #[test]
    fn validate_page_non_advancing_cursor_errors() {
        let mut expected_total = None;
        let mut last_index = Some(5);

        // cursor points backward
        let back = cursor_at(5);
        let info = page_info(3, 10, Some(&back));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_err());

        // cursor stays at same index
        let same = cursor_at(5);
        let info = page_info(3, 10, Some(&same));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_err());

        // cursor advances — ok
        let forward = cursor_at(8);
        let info = page_info(3, 10, Some(&forward));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_ok());
    }

    #[test]
    fn validate_page_undecodable_cursor_errors() {
        let mut expected_total = None;
        let mut last_index = None;
        let info = page_info(3, 10, Some("not-a-cursor"));
        assert!(validate_page(&info, &mut expected_total, &mut last_index, "test").is_err());
    }

    // BlockAndReceiptsReducer reducer

    #[test]
    fn receipts_reducer_two_pages_reassemble() {
        let full_block = external_block_with_txs(5);
        let (page1_block, page2_block) = split_external_block(&full_block, 3);
        let block_number = full_block.number();

        let mut reducer = BlockAndReceiptsReducer::new(block_number);

        let page1 = receipts_page(page1_block, vec![Faker.fake(); 3], page_info(3, 5, Some(&cursor_at(3))));
        let cursor = reducer.reduce(page1).expect("ok");
        assert_eq!(cursor, Some(cursor_at(3)));

        let page2 = receipts_page(page2_block, vec![Faker.fake(); 2], page_info(2, 5, None));
        let cursor = reducer.reduce(page2).expect("ok");
        assert!(cursor.is_none());

        let result = reducer.finish().expect("ok").expect("some output");
        assert_eq!(result.block.try_full_transactions_len().unwrap(), 5);
        assert_eq!(result.receipts.len(), 5);
    }

    #[test]
    fn receipts_reducer_wrong_block_number_errors() {
        let block = external_block_with_txs(2);
        let block_number = BlockNumber::from(999u64);

        let mut reducer = BlockAndReceiptsReducer::new(block_number);
        let page = receipts_page(block, vec![], page_info(0, 0, None));
        assert!(reducer.reduce(page).is_err());
    }

    #[test]
    fn receipts_reducer_count_mismatch_errors() {
        let block = external_block_with_txs(3);
        let block_number = block.number();

        let mut reducer = BlockAndReceiptsReducer::new(block_number);
        let page = receipts_page(block, vec![Faker.fake(); 2], page_info(3, 3, None));
        reducer.reduce(page).expect("ok");

        assert!(reducer.finish().is_err());
    }

    #[test]
    fn receipts_reducer_finish_after_not_found_no_block_returns_none() {
        let reducer = BlockAndReceiptsReducer::new(BlockNumber::from(1u64));
        assert!(reducer.finish_after_not_found().expect("ok").is_none());
    }

    #[test]
    fn receipts_reducer_finish_after_not_found_with_partial_block_errors() {
        let block = external_block_with_txs(1);
        let mut reducer = BlockAndReceiptsReducer::new(block.number());
        let page = receipts_page(block, vec![Faker.fake()], page_info(1, 1, Some(&cursor_at(1))));
        reducer.reduce(page).expect("ok");

        assert!(reducer.finish_after_not_found().is_err());
    }

    // Backward compatibility: legacy responses (no pagination field) from old leaders

    #[test]
    fn receipts_reducer_legacy_single_page_without_pagination() {
        // Old leader returns the whole block in one go without a pagination field.
        let block = external_block_with_txs(3);
        let receipts: Vec<ExternalReceipt> = vec![Faker.fake(); 3];
        let block_number = block.number();

        let mut reducer = BlockAndReceiptsReducer::new(block_number);
        let page = BlockAndReceiptsPageResponse {
            block: block.clone(),
            receipts,
            pagination: None,
        };

        // No cursor -> the fetcher treats this as the final (single) page.
        let cursor = reducer.reduce(page).expect("ok");
        assert!(cursor.is_none());

        // finish() must succeed without a reported total and without slicing expectations.
        let result = reducer.finish().expect("ok").expect("some output");
        assert_eq!(result.block.try_full_transactions_len().unwrap(), 3);
        assert_eq!(result.receipts.len(), 3);
    }

    #[test]
    fn receipts_reducer_legacy_page_block_number_mismatch_still_errors() {
        let block = external_block_with_txs(1);
        let mut reducer = BlockAndReceiptsReducer::new(BlockNumber::from(999u64));
        let page = BlockAndReceiptsPageResponse {
            block,
            receipts: vec![Faker.fake()],
            pagination: None,
        };

        assert!(reducer.reduce(page).is_err());
    }

    #[test]
    fn receipts_reducer_legacy_tx_receipt_count_mismatch_still_errors() {
        let block = external_block_with_txs(2);
        let mut reducer = BlockAndReceiptsReducer::new(block.number());
        let page = BlockAndReceiptsPageResponse {
            block,
            receipts: vec![Faker.fake()],
            pagination: None,
        };
        reducer.reduce(page).expect("ok");

        assert!(reducer.finish().is_err());
    }

    // BlockWithChangesReducer reducer

    #[test]
    fn changes_reducer_two_pages_reassemble() {
        let block_number = BlockNumber::from(1u64);
        let full_block = block_rocksdb_with_txs(1, 3);
        let (page1_block, page2_block) = {
            let txs = full_block.transactions.clone();
            let mut p1 = full_block.clone();
            let mut p2 = full_block.clone();
            p1.transactions = txs[..1].to_vec();
            p2.transactions = txs[1..].to_vec();
            (p1, p2)
        };

        let addr_a = AddressRocksdb([0x01; 20]);
        let addr_b = AddressRocksdb([0x02; 20]);
        let changes1 = changes_with_accounts(&[addr_a.0]);
        let changes2 = changes_with_accounts(&[addr_b.0]);

        let mut reducer = BlockWithChangesReducer::new(block_number);

        let page1 = BlockWithChangesPageResponse {
            block: page1_block,
            changes: changes1,
            pagination: Some(page_info(2, 5, Some(&cursor_at(2)))),
        };
        let cursor = reducer.reduce(page1).expect("ok");
        assert_eq!(cursor, Some(cursor_at(2)));

        let page2 = BlockWithChangesPageResponse {
            block: page2_block,
            changes: changes2,
            pagination: Some(page_info(3, 5, None)),
        };
        let cursor = reducer.reduce(page2).expect("ok");
        assert!(cursor.is_none());

        let (block, changes) = reducer.finish().expect("ok").expect("some output");
        assert_eq!(block.transactions.len(), 3);
        assert_eq!(changes.account_changes.len(), 2);
    }

    #[test]
    fn changes_reducer_block_number_mismatch_errors() {
        let block = block_rocksdb_with_txs(1, 1);
        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(999u64));
        let page = BlockWithChangesPageResponse {
            block,
            changes: BlockChangesRocksdb::default(),
            pagination: Some(page_info(1, 1, None)),
        };
        assert!(reducer.reduce(page).is_err());
    }

    #[test]
    fn changes_reducer_hash_changed_between_pages_errors() {
        let block1 = block_rocksdb_with_txs(1, 1);
        let mut block2 = block_rocksdb_with_txs(1, 1);
        block2.header.hash = Faker.fake();

        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));

        let page1 = BlockWithChangesPageResponse {
            block: block1,
            changes: BlockChangesRocksdb::default(),
            pagination: Some(page_info(1, 2, Some(&cursor_at(1)))),
        };
        reducer.reduce(page1).expect("ok");

        let page2 = BlockWithChangesPageResponse {
            block: block2,
            changes: BlockChangesRocksdb::default(),
            pagination: Some(page_info(1, 2, None)),
        };
        assert!(reducer.reduce(page2).is_err());
    }

    #[test]
    fn changes_reducer_duplicate_account_change_errors() {
        let block = block_rocksdb_with_txs(1, 0);
        let addr = [0x01; 20];
        let changes = changes_with_accounts(&[addr]);

        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));

        let page1 = BlockWithChangesPageResponse {
            block: block.clone(),
            changes: changes.clone(),
            pagination: Some(page_info(1, 2, Some(&cursor_at(1)))),
        };
        reducer.reduce(page1).expect("ok");

        let page2 = BlockWithChangesPageResponse {
            block,
            changes,
            pagination: Some(page_info(1, 2, None)),
        };
        assert!(reducer.reduce(page2).is_err());
    }

    #[test]
    fn changes_reducer_duplicate_slot_change_errors() {
        let block = block_rocksdb_with_txs(1, 0);
        let addr = AddressRocksdb([0x01; 20]);
        let idx = slot_idx(0, 0, 0, 1);
        let changes = changes_with_slots(&[(addr, idx)]);

        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));

        let page1 = BlockWithChangesPageResponse {
            block: block.clone(),
            changes: changes.clone(),
            pagination: Some(page_info(1, 2, Some(&cursor_at(1)))),
        };
        reducer.reduce(page1).expect("ok");

        let page2 = BlockWithChangesPageResponse {
            block,
            changes,
            pagination: Some(page_info(1, 2, None)),
        };
        assert!(reducer.reduce(page2).is_err());
    }

    #[test]
    fn changes_reducer_total_mismatch_errors() {
        let block = block_rocksdb_with_txs(1, 2);
        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));

        let page = BlockWithChangesPageResponse {
            block,
            changes: BlockChangesRocksdb::default(),
            pagination: Some(page_info(2, 10, None)),
        };
        reducer.reduce(page).expect("ok");

        assert!(reducer.finish().is_err());
    }

    #[test]
    fn changes_reducer_finish_after_not_found_no_block_returns_none() {
        let reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));
        assert!(reducer.finish_after_not_found().expect("ok").is_none());
    }

    #[test]
    fn changes_reducer_finish_after_not_found_with_partial_block_errors() {
        let block = block_rocksdb_with_txs(1, 1);
        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));
        let page = BlockWithChangesPageResponse {
            block,
            changes: BlockChangesRocksdb::default(),
            pagination: Some(page_info(1, 1, Some(&cursor_at(1)))),
        };
        reducer.reduce(page).expect("ok");

        assert!(reducer.finish_after_not_found().is_err());
    }

    // Backward compatibility: legacy responses (no pagination field) from old leaders

    #[test]
    fn changes_reducer_legacy_single_page_without_pagination() {
        // Old leader returns the whole block and changes in one go without a pagination field.
        let block_number = BlockNumber::from(1u64);
        let block = block_rocksdb_with_txs(1, 3);
        let changes = changes_with_accounts(&[[0x01; 20], [0x02; 20]]);

        let mut reducer = BlockWithChangesReducer::new(block_number);
        let page = BlockWithChangesPageResponse {
            block,
            changes,
            pagination: None,
        };

        // No cursor -> the fetcher treats this as the final (single) page.
        let cursor = reducer.reduce(page).expect("ok");
        assert!(cursor.is_none());

        // finish() must succeed without a reported total.
        let (block, changes) = reducer.finish().expect("ok").expect("some output");
        assert_eq!(block.transactions.len(), 3);
        assert_eq!(changes.account_changes.len(), 2);
    }

    #[test]
    fn changes_reducer_legacy_page_block_number_mismatch_still_errors() {
        let block = block_rocksdb_with_txs(1, 1);
        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(999u64));
        let page = BlockWithChangesPageResponse {
            block,
            changes: BlockChangesRocksdb::default(),
            pagination: None,
        };

        assert!(reducer.reduce(page).is_err());
    }

    #[test]
    fn changes_reducer_legacy_duplicate_account_change_not_applicable_in_single_page() {
        // Legacy responses arrive as a single page; duplicate detection is exercised by
        // paginated responses only. Just assert single-page reduce+finish succeeds here.
        let block = block_rocksdb_with_txs(1, 0);
        let changes = changes_with_accounts(&[[0x01; 20]]);

        let mut reducer = BlockWithChangesReducer::new(BlockNumber::from(1u64));
        let page = BlockWithChangesPageResponse {
            block,
            changes,
            pagination: None,
        };
        assert!(reducer.reduce(page).is_ok());
        assert!(reducer.finish().is_ok());
    }
}
