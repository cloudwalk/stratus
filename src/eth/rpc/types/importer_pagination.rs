use std::collections::HashMap;
use std::ops::Range;

use jsonrpsee::types::ParamsSequence;

use super::BlockFilter;
use super::RpcError;
use super::pagination::CursorCodec;
use super::pagination::CursorPageInfo;
use super::pagination::CursorPaginator;
use super::pagination::Paginator;
use super::pagination::PaginatorConfig;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImporterPageRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

impl ImporterPageRequest {
    fn parse_optional(mut params: ParamsSequence<'_>) -> Result<Option<Self>, RpcError> {
        match params.optional_next::<Self>() {
            Ok(page_request) => Ok(page_request),
            Err(e) => Err(RpcError::ParameterDecodeError {
                rust_type: "ImporterPageRequest",
                decode_error: e.data().map(|x| x.to_string()).unwrap_or_default(),
            }),
        }
    }

    fn limit(&self) -> usize {
        match self.limit {
            Some(0) | None => IMPORTER_PAGE_LIMIT_DEFAULT,
            Some(limit) => limit.min(IMPORTER_PAGE_LIMIT_MAX),
        }
    }
}

pub struct ImporterPagination {
    limit: usize,
    start: usize,
}

impl ImporterPagination {
    /// Parses optional pagination parameters from the RPC params sequence.
    ///
    /// Returns `None` if no pagination parameter was provided (non-paginated request).
    /// Returns `Some((filter, pagination))` where `filter` is:
    /// - The original `filter` if no cursor is present (first page).
    /// - `BlockFilter::Hash(cursor.block_hash)` if a cursor is present (subsequent pages),
    ///   overriding the original filter to ensure the same block is paginated.
    pub fn from_params(params: ParamsSequence<'_>, filter: BlockFilter) -> Result<Option<(BlockFilter, Self)>, RpcError> {
        let Some(request) = ImporterPageRequest::parse_optional(params)? else {
            return Ok(None);
        };
        let (filter, start) = Self::resolve_filter(filter, request.cursor.as_deref())?;
        Ok(Some((filter, Self { limit: request.limit(), start })))
    }

    /// Test-only constructor that builds a pagination with the given start index and limit.
    #[cfg(test)]
    pub(crate) fn for_test(start: usize, limit: usize) -> Self {
        Self { limit, start }
    }

    pub fn block_and_receipts_response(&self, mut block: Block) -> Result<BlockAndReceiptsPageResponse, RpcError> {
        let mut paginator = self.build_paginator(block.transactions.len(), block.hash())?;
        let tx_range = paginator.take(block.transactions.len());
        let transactions = block.transactions[tx_range].to_vec();
        let receipts = transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect::<Vec<_>>();

        block.transactions = transactions;

        Ok(BlockAndReceiptsPageResponse {
            block: block.to_json_rpc_with_full_transactions(),
            receipts,
            pagination: Some(paginator.finish()),
        })
    }

    pub fn block_with_changes_response(&self, block: BlockRocksdb, changes: BlockChangesRocksdb) -> Result<BlockWithChangesPageResponse, RpcError> {
        let BlockRocksdb { header, transactions } = block;
        let total = transactions.len() + changes.account_changes.len() + changes.slot_changes.len();
        let mut paginator = self.build_paginator(total, header.hash.into())?;

        let tx_range = paginator.take(transactions.len());
        let account_entries = sorted_account_changes(&changes);
        let account_range = paginator.take(account_entries.len());
        let slot_entries = sorted_slot_changes(&changes);
        let slot_range = paginator.take(slot_entries.len());

        let page_changes = BlockChangesRocksdb {
            account_changes: slice_account_changes(&account_entries, account_range),
            slot_changes: slice_slot_changes(&slot_entries, slot_range),
        };

        Ok(BlockWithChangesPageResponse {
            block: BlockRocksdb {
                header,
                transactions: transactions[tx_range].to_vec(),
            },
            changes: page_changes,
            pagination: Some(paginator.finish()),
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

    fn build_paginator(&self, total: usize, block_hash: Hash) -> Result<ImporterCursorPaginator, RpcError> {
        ImporterCursorPaginator::new(
            PaginatorConfig {
                total,
                start: self.start,
                limit: self.limit,
            },
            BlockHashCursor { block_hash },
        )
        .ok_or(RpcError::ParameterInvalid)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockAndReceiptsPageResponse {
    pub block: JsonValue,
    pub receipts: Vec<ExternalReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pagination: Option<CursorPageInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockWithChangesPageResponse {
    pub block: BlockRocksdb,
    pub changes: BlockChangesRocksdb,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<CursorPageInfo>,
}

impl<'de> serde::Deserialize<'de> for BlockWithChangesPageResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ObjectForm {
            block: BlockRocksdb,
            changes: BlockChangesRocksdb,
            #[serde(default)]
            pagination: Option<CursorPageInfo>,
        }

        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Both {
            Object(ObjectForm),
            Array(BlockRocksdb, BlockChangesRocksdb),
        }

        match Both::deserialize(deserializer)? {
            Both::Object(obj) => Ok(Self {
                block: obj.block,
                changes: obj.changes,
                pagination: obj.pagination,
            }),
            Both::Array(block, changes) => Ok(Self {
                block,
                changes,
                pagination: None,
            }),
        }
    }
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

fn slice_account_changes(
    entries: &[(AddressRocksdb, AccountChangesRocksdb)],
    range: Range<usize>,
) -> HashMap<AddressRocksdb, AccountChangesRocksdb, hash_hasher::HashBuildHasher> {
    let mut changes = HashMap::with_capacity_and_hasher(range.len(), hash_hasher::HashBuildHasher::default());
    for (address, change) in entries[range].iter().cloned() {
        changes.insert(address, change);
    }
    changes
}

fn slice_slot_changes(
    entries: &[((AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb)],
    range: Range<usize>,
) -> HashMap<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb, hash_hasher::HashBuildHasher> {
    let mut changes = HashMap::with_capacity_and_hasher(range.len(), hash_hasher::HashBuildHasher::default());
    for ((address, slot), value) in entries[range].iter().copied() {
        changes.insert((address, slot), value);
    }
    changes
}

type ImporterCursorPaginator = CursorPaginator<BlockHashCursor>;

struct BlockHashCursor {
    block_hash: Hash,
}

impl CursorCodec for BlockHashCursor {
    type Error = RpcError;

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
    use crate::alias::AlloyReceipt;
    use crate::eth::rpc::BlockFilter;
    use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::ExternalReceipt;
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
        assert!(BlockHashCursor::decode_cursor("").is_err());
        assert!(BlockHashCursor::decode_cursor("v1").is_err());
        assert!(BlockHashCursor::decode_cursor("v1:0xabc").is_err());
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
        assert!(BlockHashCursor::decode_cursor("v1:0xnotahash:0").is_err());
        assert!(BlockHashCursor::decode_cursor("v1:0x1234:0").is_err());
    }

    #[test]
    fn encode_uses_v1_format() {
        let c = codec("0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c");
        let encoded = c.encode_cursor(7);
        assert!(encoded.starts_with("v1:"));
        assert_eq!(encoded.split(':').count(), 3);
    }

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
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 3);
        assert_eq!(info.total, 3);
        assert_eq!(response.receipts.len(), 3);
        assert!(info.next_cursor.is_none());
    }

    #[test]
    fn block_and_receipts_multi_page_slices_correctly() {
        let block = block_with_txs(5);

        // page 1: start=0, limit=3
        let pagination = ImporterPagination::for_test(0, 3);
        let response = pagination.block_and_receipts_response(block.clone()).expect("ok");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 3);
        assert_eq!(info.total, 5);
        assert_eq!(response.receipts.len(), 3);
        let cursor = info.next_cursor.expect("more pages");

        // decode cursor -> start=3
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 3);

        // page 2: start=3, limit=3
        let pagination = ImporterPagination::for_test(start, 3);
        let response = pagination.block_and_receipts_response(block).expect("ok");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 2);
        assert_eq!(info.total, 5);
        assert_eq!(response.receipts.len(), 2);
        assert!(info.next_cursor.is_none());
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
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 3);
        assert_eq!(info.total, 7);
        assert_eq!(response.block.transactions.len(), 2);
        assert_eq!(response.changes.account_changes.len(), 1);
        assert_eq!(response.changes.slot_changes.len(), 0);
        let cursor = info.next_cursor.expect("more pages");
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 3);

        // page 2: start=3, limit=3 -> 2 accounts + 1 slot
        let pagination = ImporterPagination::for_test(3, 3);
        let response = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 3);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 2);
        assert_eq!(response.changes.slot_changes.len(), 1);
        let cursor = info.next_cursor.expect("more pages");
        let (_, start) = BlockHashCursor::decode_cursor(&cursor).expect("valid cursor");
        assert_eq!(start, 6);

        // page 3: start=6, limit=3 -> 1 slot
        let pagination = ImporterPagination::for_test(6, 3);
        let response = pagination.block_with_changes_response(block, changes).expect("ok");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 0);
        assert_eq!(response.changes.slot_changes.len(), 1);
        assert!(info.next_cursor.is_none());
    }

    #[test]
    fn block_with_changes_single_page_returns_all() {
        let block = block_rocksdb_with_txs(1);
        let changes = changes_fixture();
        // total = 1 + 3 + 2 = 6

        let pagination = ImporterPagination::for_test(0, 10);
        let response = pagination.block_with_changes_response(block, changes).expect("ok");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 6);
        assert_eq!(info.total, 6);
        assert_eq!(response.block.transactions.len(), 1);
        assert_eq!(response.changes.account_changes.len(), 3);
        assert_eq!(response.changes.slot_changes.len(), 2);
        assert!(info.next_cursor.is_none());
    }

    // -------------------------------------------------------------------------
    // Backward compatibility: legacy (old leader without pagination) responses
    // -------------------------------------------------------------------------

    use super::BlockAndReceiptsPageResponse;
    use super::BlockWithChangesPageResponse;

    #[test]
    fn receipts_response_deserializes_legacy_object_without_pagination() {
        let block = block_with_txs(1);
        let receipts: Vec<ExternalReceipt> = block.transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect();

        // Shape returned by an old leader's stratus_getBlockAndReceipts: no `pagination` key.
        let legacy = serde_json::json!({
            "block": block.to_json_rpc_with_full_transactions(),
            "receipts": receipts,
        });

        let response: BlockAndReceiptsPageResponse = serde_json::from_value(legacy).expect("legacy response deserializes");
        assert!(response.pagination.is_none());
        assert_eq!(response.receipts.len(), 1);
    }

    #[test]
    fn receipts_response_deserializes_paginated_object() {
        let block = block_with_txs(2);
        let pagination = ImporterPagination::for_test(0, 10);
        let paginated = pagination.block_and_receipts_response(block).expect("ok");

        let json = serde_json::to_value(&paginated).expect("serialize");
        assert!(json.get("pagination").is_some());

        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated response deserializes");
        assert!(response.pagination.is_some());
    }

    #[test]
    fn receipts_response_omits_pagination_key_when_serializing_none() {
        let response = BlockAndReceiptsPageResponse {
            block: serde_json::json!({}),
            receipts: vec![],
            pagination: None,
        };
        let json = serde_json::to_value(&response).expect("serialize");
        assert!(json.get("pagination").is_none());
    }

    #[test]
    fn changes_response_deserializes_legacy_tuple_array() {
        let block = block_rocksdb_with_txs(2);
        let changes = changes_fixture();

        // Shape returned by an old leader's stratus_getBlockWithChanges: bare [block, changes] tuple.
        let legacy = serde_json::json!([block, changes]);

        let response: BlockWithChangesPageResponse = serde_json::from_value(legacy).expect("legacy tuple deserializes");
        assert!(response.pagination.is_none());
        assert_eq!(response.block, block);
        assert_eq!(response.changes, changes);
    }

    #[test]
    fn changes_response_deserializes_object_without_pagination() {
        let block = block_rocksdb_with_txs(2);
        let changes = changes_fixture();

        // Defensive shape: object form but no pagination key.
        let json = serde_json::json!({
            "block": block,
            "changes": changes,
        });

        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("object without pagination deserializes");
        assert!(response.pagination.is_none());
        assert_eq!(response.block, block);
        assert_eq!(response.changes, changes);
    }

    #[test]
    fn changes_response_deserializes_paginated_object() {
        let block = block_rocksdb_with_txs(1);
        let changes = changes_fixture();
        let pagination = ImporterPagination::for_test(0, 10);
        let paginated = pagination.block_with_changes_response(block, changes).expect("ok");

        let json = serde_json::to_value(&paginated).expect("serialize");
        assert!(json.get("pagination").is_some());

        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated response deserializes");
        assert!(response.pagination.is_some());
    }
}
