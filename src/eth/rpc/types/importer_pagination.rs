use jsonrpsee::types::ParamsSequence;

use super::BlockFilter;
use super::RpcError;
use super::pagination::CursorCodec;
use super::pagination::CursorPageInfo;
use super::pagination::CursorPaginator;
use super::pagination::Paginator;
use crate::alias::AlloyReceipt;
use crate::alias::JsonValue;
use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
use crate::eth::types::Block;
use crate::eth::types::ExternalReceipt;
use crate::eth::types::Hash;

pub const IMPORTER_PAGE_LIMIT_DEFAULT: usize = 256;
pub const IMPORTER_PAGE_LIMIT_MAX: usize = 5_000;

const IMPORTER_CURSOR_VERSION: &str = "v1";

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImporterPageRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

impl ImporterPageRequest {
    fn parse_next(mut params: ParamsSequence<'_>) -> Result<Option<Self>, RpcError> {
        match params.optional_next::<Self>() {
            Ok(page_request) => Ok(page_request),
            Err(e) => Err(RpcError::ParameterDecodeError {
                rust_type: "ImporterPageRequest",
                decode_error: e.data().map(|x| x.to_string()).unwrap_or_default(),
            }),
        }
    }

    pub(crate) fn limit(&self) -> usize {
        match self.limit {
            Some(0) | None => IMPORTER_PAGE_LIMIT_DEFAULT,
            Some(limit) => limit.min(IMPORTER_PAGE_LIMIT_MAX),
        }
    }
}

pub struct ImporterPagination {
    request: ImporterPageRequest,
    start: usize,
}

impl ImporterPagination {
    pub fn from_params(params: ParamsSequence<'_>, filter: BlockFilter) -> Result<Option<(BlockFilter, Self)>, RpcError> {
        let Some(request) = ImporterPageRequest::parse_next(params)? else {
            return Ok(None);
        };
        let (filter, start) = Self::resolve_filter(filter, request.cursor.as_deref())?;
        Ok(Some((filter, Self { request, start })))
    }

    /// Test-only constructor that builds a pagination with the given start index and limit.
    #[cfg(test)]
    pub(crate) fn for_test(start: usize, limit: usize) -> Self {
        Self {
            request: ImporterPageRequest {
                cursor: None,
                limit: Some(limit),
            },
            start,
        }
    }

    pub fn block_and_receipts_response(&self, block: Block) -> Result<BlockAndReceiptsPageResponse, RpcError> {
        let mut block = block;
        let mut paginator = self.cursor_paginator(block.transactions.len(), block.hash())?;
        let tx_range = paginator.take(block.transactions.len());
        let transactions = block.transactions[tx_range].to_vec();
        let receipts = transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect::<Vec<_>>();

        block.transactions = transactions;

        Ok(BlockAndReceiptsPageResponse {
            block: block.to_json_rpc_with_full_transactions(),
            receipts,
            pagination: paginator.finish(),
        })
    }

    pub fn block_with_changes_response(&self, block: BlockRocksdb, changes: BlockChangesRocksdb) -> Result<BlockWithChangesPageResponse, RpcError> {
        let BlockRocksdb { header, transactions } = block;
        let total = transactions.len() + changes.account_changes.len() + changes.slot_changes.len();
        let mut paginator = self.cursor_paginator(total, header.hash.into())?;

        let tx_range = paginator.take(transactions.len());
        let account_entries = sorted_account_changes(&changes);
        let account_range = paginator.take(account_entries.len());
        let slot_entries = sorted_slot_changes(&changes);
        let slot_range = paginator.take(slot_entries.len());

        let mut page_changes = BlockChangesRocksdb::with_capacity(account_range.len());
        for (address, change) in account_entries[account_range].iter().cloned() {
            page_changes.account_changes.insert(address, change);
        }
        for ((address, slot), value) in slot_entries[slot_range].iter().copied() {
            page_changes.slot_changes.insert((address, slot), value);
        }

        Ok(BlockWithChangesPageResponse {
            block: BlockRocksdb {
                header,
                transactions: transactions[tx_range].to_vec(),
            },
            changes: page_changes,
            pagination: paginator.finish(),
        })
    }

    fn resolve_filter(filter: BlockFilter, cursor: Option<&str>) -> Result<(BlockFilter, usize), RpcError> {
        match cursor {
            Some(cursor) => {
                let (cursor, next_index) = BlockHashCursor::decode_cursor(cursor)?;
                Ok((BlockFilter::Hash(cursor.block_hash), next_index))
            }
            None => Ok((filter, 0)),
        }
    }

    fn cursor_paginator(&self, total: usize, block_hash: Hash) -> Result<ImporterCursorPaginator, RpcError> {
        ImporterCursorPaginator::new(total, self.start, self.request.limit(), BlockHashCursor { block_hash })
    }
}

pub type ImporterPageInfo = CursorPageInfo;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockAndReceiptsPageResponse {
    pub block: JsonValue,
    pub receipts: Vec<ExternalReceipt>,
    pub pagination: ImporterPageInfo,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockWithChangesPageResponse {
    pub block: BlockRocksdb,
    pub changes: BlockChangesRocksdb,
    pub pagination: ImporterPageInfo,
}

fn sorted_account_changes(changes: &BlockChangesRocksdb) -> Vec<(AddressRocksdb, AccountChangesRocksdb)> {
    let mut entries = changes
        .account_changes
        .iter()
        .map(|(address, change)| (*address, change.clone()))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(address, _)| *address);
    entries
}

fn sorted_slot_changes(changes: &BlockChangesRocksdb) -> Vec<((AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb)> {
    let mut entries = changes.slot_changes.iter().map(|(key, value)| (*key, *value)).collect::<Vec<_>>();
    entries.sort_by_key(|(key, _)| *key);
    entries
}

type ImporterCursorPaginator = CursorPaginator<BlockHashCursor>;

pub(crate) struct BlockHashCursor {
    block_hash: Hash,
}

impl CursorCodec for BlockHashCursor {
    type Error = RpcError;

    fn invalid_start_error() -> Self::Error {
        RpcError::ParameterInvalid
    }

    fn encode_cursor(&self, next_index: usize) -> String {
        format!("{IMPORTER_CURSOR_VERSION}:{}:{next_index}", self.block_hash)
    }

    fn decode_cursor(cursor: &str) -> Result<(Self, usize), Self::Error> {
        let mut parts = cursor.split(':');
        let version = parts.next().ok_or(RpcError::ParameterInvalid)?;
        let block_hash = parts.next().ok_or(RpcError::ParameterInvalid)?;
        let next_index = parts.next().ok_or(RpcError::ParameterInvalid)?;

        if version != IMPORTER_CURSOR_VERSION || parts.next().is_some() {
            return Err(RpcError::ParameterInvalid);
        }

        Ok((
            Self {
                block_hash: block_hash.parse().map_err(|_| RpcError::ParameterInvalid)?,
            },
            next_index.parse().map_err(|_| RpcError::ParameterInvalid)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fake::Fake;
    use fake::Faker;
    use hash_hasher::HashBuildHasher;
    use jsonrpsee::types::Params;

    use super::BlockHashCursor;
    use super::CursorCodec;
    use super::IMPORTER_PAGE_LIMIT_DEFAULT;
    use super::IMPORTER_PAGE_LIMIT_MAX;
    use super::ImporterPageRequest;
    use super::ImporterPagination;
    use super::RpcError;
    use crate::eth::rpc::BlockFilter;
    use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::Hash;
    use crate::eth::types::SlotIndex;
    use crate::eth::types::SlotValue;
    use crate::eth::types::UnixTime;

    fn codec(block_hash: &str) -> BlockHashCursor {
        BlockHashCursor {
            block_hash: block_hash.parse().unwrap(),
        }
    }

    // -------------------------------------------------------------------------
    // Cursor codec tests
    // -------------------------------------------------------------------------

    #[test]
    fn cursor_roundtrip_preserves_hash_and_index() {
        let original = codec("0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c");
        let encoded = original.encode_cursor(42);
        let (decoded, next_index) = BlockHashCursor::decode_cursor(&encoded).expect("decode succeeds");

        assert_eq!(decoded.block_hash, original.block_hash);
        assert_eq!(next_index, 42);
    }

    #[test]
    fn cursor_decode_rejects_wrong_version() {
        let bad = "v2:0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c:0";
        let err = BlockHashCursor::decode_cursor(bad).err();
        assert!(matches!(err, Some(RpcError::ParameterInvalid)));
    }

    #[test]
    fn cursor_decode_rejects_missing_parts() {
        assert!(BlockHashCursor::decode_cursor("v1:0xabc").is_err());
        assert!(BlockHashCursor::decode_cursor("v1").is_err());
    }

    #[test]
    fn cursor_decode_rejects_extra_parts() {
        let extra = "v1:0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c:0:extra";
        assert!(BlockHashCursor::decode_cursor(extra).is_err());
    }

    #[test]
    fn cursor_decode_rejects_non_numeric_index() {
        let bad = "v1:0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c:notanumber";
        assert!(BlockHashCursor::decode_cursor(bad).is_err());
    }

    #[test]
    fn cursor_decode_rejects_invalid_hash() {
        let bad = "v1:0xnotahash:0";
        assert!(BlockHashCursor::decode_cursor(bad).is_err());
    }

    #[test]
    fn encode_uses_v1_format() {
        let c = codec("0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c");
        let encoded = c.encode_cursor(7);
        assert!(encoded.starts_with("v1:"));
        assert_eq!(encoded.split(':').count(), 3);
    }

    // ensure Hash parses from a 0x-prefixed hex string in tests
    #[test]
    fn hash_parses_for_test_fixture() {
        let _: Hash = "0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c".parse().unwrap();
    }

    // ImporterPageRequest::limit()

    #[test]
    fn limit_none_returns_default() {
        let req = ImporterPageRequest { cursor: None, limit: None };
        assert_eq!(req.limit(), IMPORTER_PAGE_LIMIT_DEFAULT);
    }

    #[test]
    fn limit_zero_returns_default() {
        let req = ImporterPageRequest { cursor: None, limit: Some(0) };
        assert_eq!(req.limit(), IMPORTER_PAGE_LIMIT_DEFAULT);
    }

    #[test]
    fn limit_small_value_passed_through() {
        let req = ImporterPageRequest { cursor: None, limit: Some(50) };
        assert_eq!(req.limit(), 50);
    }

    #[test]
    fn limit_large_value_clamped_to_max() {
        let req = ImporterPageRequest {
            cursor: None,
            limit: Some(10_000),
        };
        assert_eq!(req.limit(), IMPORTER_PAGE_LIMIT_MAX);
    }

    // ImporterPagination::from_params

    #[test]
    fn from_params_no_pagination_param_returns_none() {
        let params = Params::new(Some("[]"));
        let result = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest).expect("ok");
        assert!(result.is_none());
    }

    #[test]
    fn from_params_with_cursor_resolves_hash_filter_and_start() {
        let block_hash = "0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c";
        let cursor = codec(block_hash).encode_cursor(5);
        let json = format!(r#"[{{"cursor":"{cursor}","limit":10}}]"#);

        let params = Params::new(Some(&json));
        let (filter, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest)
            .expect("ok")
            .expect("pagination present");

        assert!(matches!(filter, BlockFilter::Hash(h) if h == block_hash.parse::<Hash>().unwrap()));
        assert_eq!(pagination.start, 5);
    }

    // block_and_receipts_response (server slicing)

    fn block_with_txs(count: usize) -> Block {
        let mut block = Block::new(BlockNumber::from(1u64), UnixTime::from(0u64));
        block.transactions = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        block
    }

    #[test]
    fn block_and_receipts_single_page_returns_all() {
        let block = block_with_txs(3);
        let pagination = ImporterPagination::for_test(0, 10);
        let response = pagination.block_and_receipts_response(block).expect("ok");

        assert_eq!(response.pagination.returned, 3);
        assert_eq!(response.pagination.total, 3);
        assert_eq!(response.receipts.len(), 3);
        assert!(response.pagination.next_cursor.is_none());
    }

    #[test]
    fn block_and_receipts_multi_page_slices_correctly() {
        let block = block_with_txs(5);

        // page 1: start=0, limit=3
        let pagination = ImporterPagination::for_test(0, 3);
        let response = pagination.block_and_receipts_response(block.clone()).expect("ok");

        assert_eq!(response.pagination.returned, 3);
        assert_eq!(response.pagination.total, 5);
        assert_eq!(response.receipts.len(), 3);
        let cursor = response.pagination.next_cursor.expect("more pages");

        // decode cursor -> start=3
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 3);

        // page 2: start=3, limit=3
        let pagination = ImporterPagination::for_test(start, 3);
        let response = pagination.block_and_receipts_response(block).expect("ok");

        assert_eq!(response.pagination.returned, 2);
        assert_eq!(response.pagination.total, 5);
        assert_eq!(response.receipts.len(), 2);
        assert!(response.pagination.next_cursor.is_none());
    }

    // block_with_changes_response (3-section slicing)

    fn changes_fixture() -> BlockChangesRocksdb {
        let mut account_changes = HashMap::with_hasher(HashBuildHasher::default());
        account_changes.insert(AddressRocksdb([0x01; 20]), AccountChangesRocksdb::default());
        account_changes.insert(AddressRocksdb([0x02; 20]), AccountChangesRocksdb::default());
        account_changes.insert(AddressRocksdb([0x03; 20]), AccountChangesRocksdb::default());

        let mut slot_changes = HashMap::with_hasher(HashBuildHasher::default());
        slot_changes.insert(
            (AddressRocksdb([0x01; 20]), SlotIndexRocksdb::from(SlotIndex::from([0u64, 0, 0, 1]))),
            SlotValueRocksdb::from(SlotValue::from([0u64, 0, 0, 1])),
        );
        slot_changes.insert(
            (AddressRocksdb([0x01; 20]), SlotIndexRocksdb::from(SlotIndex::from([0u64, 0, 0, 2]))),
            SlotValueRocksdb::from(SlotValue::from([0u64, 0, 0, 2])),
        );

        BlockChangesRocksdb { account_changes, slot_changes }
    }

    fn block_rocksdb_with_txs(count: usize) -> BlockRocksdb {
        let mut block = Block::new(BlockNumber::from(1u64), UnixTime::from(0u64));
        block.transactions = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        BlockRocksdb::from(block)
    }

    #[test]
    fn block_with_changes_multi_page_slices_three_sections() {
        let block = block_rocksdb_with_txs(2);
        let changes = changes_fixture();
        // total = 2 txs + 3 accounts + 2 slots = 7

        // page 1: start=0, limit=3 -> 2 txs + 1 account
        let pagination = ImporterPagination::for_test(0, 3);
        let response = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");

        assert_eq!(response.pagination.returned, 3);
        assert_eq!(response.pagination.total, 7);
        assert_eq!(response.block.transactions.len(), 2);
        assert_eq!(response.changes.account_changes.len(), 1);
        assert_eq!(response.changes.slot_changes.len(), 0);
        let cursor = response.pagination.next_cursor.expect("more pages");
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 3);

        // page 2: start=3, limit=3 -> 2 accounts + 1 slot
        let pagination = ImporterPagination::for_test(3, 3);
        let response = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");

        assert_eq!(response.pagination.returned, 3);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 2);
        assert_eq!(response.changes.slot_changes.len(), 1);
        let cursor = response.pagination.next_cursor.expect("more pages");
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 6);

        // page 3: start=6, limit=3 -> 1 slot
        let pagination = ImporterPagination::for_test(6, 3);
        let response = pagination.block_with_changes_response(block, changes).expect("ok");

        assert_eq!(response.pagination.returned, 1);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 0);
        assert_eq!(response.changes.slot_changes.len(), 1);
        assert!(response.pagination.next_cursor.is_none());
    }

    #[test]
    fn block_with_changes_single_page_returns_all() {
        let block = block_rocksdb_with_txs(1);
        let changes = changes_fixture();
        // total = 1 + 3 + 2 = 6

        let pagination = ImporterPagination::for_test(0, 10);
        let response = pagination.block_with_changes_response(block, changes).expect("ok");

        assert_eq!(response.pagination.returned, 6);
        assert_eq!(response.pagination.total, 6);
        assert_eq!(response.block.transactions.len(), 1);
        assert_eq!(response.changes.account_changes.len(), 3);
        assert_eq!(response.changes.slot_changes.len(), 2);
        assert!(response.pagination.next_cursor.is_none());
    }
}
