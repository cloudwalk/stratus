use std::collections::HashMap;
use std::ops::Range;

use alloy_rpc_types_eth::BlockTransactions;
use itertools::Itertools;
use jsonrpsee::types::ParamsSequence;
use serde_json::json;

use super::BlockFilter;
use super::RpcError;
use super::pagination::CursorCodec;
use super::pagination::CursorPageInfo;
use crate::alias::AlloyBlockAlloyTransaction;
use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
use crate::eth::types::Block;
use crate::eth::types::ExternalBlock;
use crate::eth::types::ExternalReceipt;
use crate::eth::types::ExternalTransaction;
use crate::eth::types::Hash;

pub const IMPORTER_PAGE_LIMIT_DEFAULT: usize = 256;
/// Floor for an explicit `limit`: prevents pathological page counts (e.g. `limit: 1`
/// against a big block would emit thousands of pages).
pub const IMPORTER_PAGE_LIMIT_MIN: usize = 32;
pub const IMPORTER_PAGE_LIMIT_MAX: usize = 5_000;

const IMPORTER_CURSOR_VERSION: &str = "v1";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImporterPageRequest {
    pub cursor: Option<String>,
    /// Item-count override that forces paginated, count-based responses.
    ///
    /// Not a production control: the production follower sends plain requests and
    /// lets the leader page by response size. This exists so tests and tooling can
    /// exercise multi-page flows cheaply (hundreds of items instead of multi-MB
    /// blocks). Values are clamped to [`IMPORTER_PAGE_LIMIT_MIN`]..=
    /// [`IMPORTER_PAGE_LIMIT_MAX`].
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

    /// Clamped explicit limit, or `None` when the caller sent none.
    ///
    /// Only invoked by [`ImporterPagination::from_params`] on the explicit-override
    /// path — a plain request resolves to the byte-budget policy instead.
    fn explicit_limit(&self) -> Option<usize> {
        let limit = match self.limit {
            None => return None,
            Some(0) => IMPORTER_PAGE_LIMIT_DEFAULT,
            Some(limit) => limit.min(IMPORTER_PAGE_LIMIT_MAX),
        };
        Some(limit.max(IMPORTER_PAGE_LIMIT_MIN))
    }
}

/// How a paginated response stream is sliced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PagePolicy {
    /// Page by item count (explicit `limit` override — a test/tooling knob, not a
    /// production control; see [`ImporterPageRequest::limit`]).
    Count(usize),
    /// Page by a serialized-bytes budget (production default). Measured on the
    /// exact encoding each response endpoint emits, so a page is guaranteed to fit
    /// the budget — except for a single item that alone exceeds the budget, which
    /// is still emitted to guarantee stream progress.
    Bytes(usize),
}

pub struct ImporterPagination {
    start: usize,
    policy: PagePolicy,
}

impl ImporterPagination {
    /// Reserved bytes of the response-cap budget for the JSON-RPC envelope
    /// (`{"jsonrpc":"2.0","id":..,"result":<payload>}`) when comparing an
    /// emitted response against the configured max response size.
    const ENVELOPE_RESERVE: usize = 128;

    /// Reserved bytes of a paginated page's cap budget for the `pagination`
    /// metadata: `,"pagination":{"limit":<20 digits>,"returned":<20>,
    /// "total":<20>,"nextCursor":"v1:0x<64 hex>:<20 digits>"}` ≈ 215 bytes.
    const PAGINATION_RESERVE: usize = 256;

    /// Parses pagination parameters from the RPC params sequence.
    ///
    /// Every request is potentially paginable: a plain `[filter]` request uses a byte
    /// budget derived from the server's max response size, while a request carrying
    /// `cursor` resumes from the indexed position. `limit`, when explicitly provided,
    /// switches the policy to item-count (used by tests to force pagination cheaply).
    ///
    /// Returns `(filter, pagination)` where `filter` is:
    /// - The original `filter` if no cursor is present (first page).
    /// - `BlockFilter::Hash(cursor.block_hash)` if a cursor is present (subsequent pages),
    ///   overriding the original filter to ensure the same block is paginated.
    pub fn from_params(params: ParamsSequence<'_>, filter: BlockFilter, max_response_bytes: u32) -> Result<(BlockFilter, Self), RpcError> {
        let request = ImporterPageRequest::parse_optional(params)?;
        let (filter, start) = Self::resolve_filter(filter, request.as_ref().and_then(|r| r.cursor.as_deref()))?;

        let cap = (max_response_bytes as usize).saturating_sub(Self::ENVELOPE_RESERVE);
        let policy = match request.and_then(|request| request.explicit_limit()) {
            Some(limit) => PagePolicy::Count(limit),
            None => PagePolicy::Bytes(cap),
        };

        Ok((filter, Self { start, policy }))
    }

    /// Test-only constructor that builds a count-based pagination with the given start index and limit.
    #[cfg(test)]
    pub(crate) fn for_test(start: usize, limit: usize) -> Self {
        Self {
            start,
            policy: PagePolicy::Count(limit),
        }
    }

    /// Test-only constructor that builds a byte-budget pagination.
    #[cfg(test)]
    pub(crate) fn for_budget_test(start: usize, budget: usize) -> Self {
        Self {
            start,
            policy: PagePolicy::Bytes(budget),
        }
    }

    /// Builds the response for `stratus_getBlockAndReceipts`.
    ///
    /// Paginates only when the requested page does not fit entirely in one response
    /// (a continuation always paginates, a first request only paginates when the
    /// total exceeds the policy budget). Otherwise returns the legacy one-shot
    /// `{block, receipts}` shape with no `pagination` field.
    pub fn block_and_receipts_response(&self, block: Block) -> Result<JsonValue, RpcError> {
        let Block { header, transactions } = block;
        let total = transactions.len();
        self.validate_start(total)?;

        // Convert to the wire representation up front: the byte budget must be
        // measured against the exact encoding the response carries (alloy
        // JSON-RPC), not against the internal mined encoding. Each item is
        // charged for its wire size plus the two commas that separate it from
        // its neighbors (one in `block.transactions`, one in `receipts`).
        let block_hash = header.hash;
        let receipts: Vec<ExternalReceipt> = transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect();
        let mut transactions: Vec<AlloyTransaction> = transactions.into_iter().map_into().collect();

        // Frame: the emitted block with an empty transaction list (header and
        // JSON structural bytes) plus the response keys and array brackets;
        // PAGINATION_RESERVE is charged only when the page paginates.
        let empty_block: AlloyBlockAlloyTransaction = header.clone().into();
        let base_frame = json_len(&json!({ "block": empty_block.map_transactions(ExternalTransaction::from), "receipts": Vec::<ExternalReceipt>::new() }));

        let Some((end, limit_metric)) = self.page_window(
            || {
                transactions
                    .iter()
                    .zip(&receipts)
                    .map(|(tx, receipt)| json_len(tx).saturating_add(json_len(receipt)).saturating_add(2))
                    .collect()
            },
            base_frame,
            total,
        ) else {
            let mut block: AlloyBlockAlloyTransaction = header.into();
            block.transactions = BlockTransactions::Full(transactions);
            return Ok(json!({
                "block": block.map_transactions(ExternalTransaction::from),
                "receipts": receipts,
            }));
        };

        let tx_range = self.start..end;
        let page_info = self.page_info(end, limit_metric, total, block_hash);
        let page_transactions: Vec<AlloyTransaction> = transactions.drain(tx_range.clone()).collect();

        let mut block: AlloyBlockAlloyTransaction = header.into();
        block.transactions = BlockTransactions::Full(page_transactions);
        Ok(json!(BlockAndReceiptsPageResponse {
            block: ExternalBlock(block.map_transactions(ExternalTransaction::from)),
            receipts: receipts[tx_range].to_vec(),
            pagination: Some(page_info),
        }))
    }

    /// Builds the response for `stratus_getBlockWithChanges`.
    ///
    /// Same one-shot/paginate rule as [`Self::block_and_receipts_response`]; the
    /// one-shot shape is the legacy `[block, changes]` tuple.
    pub fn block_with_changes_response(&self, block: BlockRocksdb, changes: BlockChangesRocksdb) -> Result<JsonValue, RpcError> {
        let BlockRocksdb { header, transactions } = block;
        let total = transactions.len() + changes.account_changes.len() + changes.slot_changes.len();
        self.validate_start(total)?;

        // Each item is charged for its wire size plus one comma separator.
        let account_entries = sorted_account_changes(&changes);
        let slot_entries = sorted_slot_changes(&changes);
        let section_lens = [transactions.len(), account_entries.len(), slot_entries.len()];

        // Frame: serialized `[block, changes]` tuple with empty item sections
        // (block header and JSON structural bytes).
        let base_frame = json_len(&json!((
            BlockRocksdb {
                header: header.clone(),
                transactions: Vec::new()
            },
            BlockChangesRocksdb::default()
        )));

        let Some((end, limit_metric)) = self.page_window(
            || {
                transactions
                    .iter()
                    .map(|item| json_len(item).saturating_add(1))
                    .chain(account_entries.iter().map(|entry| json_len(entry).saturating_add(1)))
                    .chain(slot_entries.iter().map(|entry| json_len(entry).saturating_add(1)))
                    .collect()
            },
            base_frame,
            total,
        ) else {
            return Ok(json!((BlockRocksdb { header, transactions }, changes)));
        };

        let ranges = slice_ranges(&section_lens, self.start, end);

        let page_changes = BlockChangesRocksdb {
            account_changes: slice_account_changes(&account_entries, ranges[1].clone()),
            slot_changes: slice_slot_changes(&slot_entries, ranges[2].clone()),
        };

        Ok(json!(BlockWithChangesPageResponse {
            block: BlockRocksdb {
                header: header.clone(),
                transactions: transactions[ranges[0].clone()].to_vec(),
            },
            changes: page_changes,
            pagination: Some(self.page_info(end, limit_metric, total, header.hash.into())),
        }))
    }

    /// Computes the end index for this page, or `None` when the whole result fits in
    /// a single (one-shot) response.
    ///
    /// Continuations (`start > 0`) always paginate; first requests only paginate when
    /// the total exceeds the policy budget (count or bytes).
    ///
    /// `base_frame` is the serialized length of everything in the response that is
    /// not an item payload (block header, JSON structural bytes). The one-shot check
    /// compares `Σ items + base_frame` against the cap directly — no headroom
    /// divisor: item sizes and the frame are now measured on the exact emitted
    /// encoding, so the accounting is closed (only the JSON-RPC envelope remains
    /// outside it, reserved up front in [`Self::ENVELOPE_RESERVE`]). Paginated pages
    /// additionally reserve [`Self::PAGINATION_RESERVE`] for the pagination metadata.
    fn page_window(&self, byte_sizes: impl FnOnce() -> Vec<usize>, base_frame: usize, total: usize) -> Option<(usize, usize)> {
        let page_frame = base_frame.saturating_add(Self::PAGINATION_RESERVE);
        if self.start > 0 {
            return Some(self.paginate_first_or_continue(byte_sizes, page_frame, total));
        }

        match self.policy {
            PagePolicy::Count(limit) => (total > limit).then(|| self.paginate_first_or_continue(byte_sizes, page_frame, total)),
            PagePolicy::Bytes(cap) => {
                let sizes = byte_sizes();
                // sizing happens even on the one-shot path: it's how we know it fits.
                // saturating: usize::MAX (serialization-failure marker) would overflow plain sum.
                let total_bytes: usize = sizes.iter().copied().fold(0usize, usize::saturating_add);
                (total_bytes.saturating_add(base_frame) > cap).then(|| self.paginate_first_or_continue(|| sizes, page_frame, total))
            }
        }
    }

    /// Computes `(end, limit_metric)` for any request that must paginate. The item
    /// budget is the cap minus the page frame.
    fn paginate_first_or_continue(&self, byte_sizes: impl FnOnce() -> Vec<usize>, page_frame: usize, total: usize) -> (usize, usize) {
        match self.policy {
            PagePolicy::Count(limit) => (self.start.saturating_add(limit).min(total), limit),
            PagePolicy::Bytes(cap) => (self.start + take_by_budget(&byte_sizes(), self.start, cap.saturating_sub(page_frame)), cap),
        }
    }

    fn page_info(&self, end: usize, limit_metric: usize, total: usize, block_hash: Hash) -> CursorPageInfo {
        CursorPageInfo {
            limit: limit_metric,
            returned: end - self.start,
            total,
            next_cursor: (end < total).then(|| BlockHashCursor::new(block_hash).encode_cursor(end)),
        }
    }

    /// Decodes a client-supplied cursor into `(block_hash, next_index)`.
    ///
    /// Used by both the server (parameter parsing) and pagination-capable RPC
    /// clients (cursor progression validation).
    pub fn decode_cursor_for_validation(cursor: &str) -> Result<(Hash, usize), RpcError> {
        let (cursor, next_index) = BlockHashCursor::decode_cursor(cursor)?;
        Ok((cursor.block_hash, next_index))
    }

    /// Rejects cursors that point beyond the available items (mirrors the semantics
    /// of the previous `CursorPaginator::new` validation).
    fn validate_start(&self, total: usize) -> Result<(), RpcError> {
        if self.start > total || (self.start == total && total != 0) {
            return Err(RpcError::ParameterInvalid);
        }
        Ok(())
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
}

/// Serialized byte length of a value (used to budget byte-based pages).
///
/// The data types sized here always serialize; on a surprise failure we count the
/// item as "largest possible" instead of zero, so the page budget overestimates
/// rather than silently dropping the item from the accounting.
fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map(|json| json.len()).unwrap_or(usize::MAX)
}

/// Number of consecutive items starting at `start` that fit into `budget`.
///
/// Always consumes at least one item: when a single item exceeds the budget the page
/// carries it anyway, so the stream can always advance past oversized items.
fn take_by_budget(sizes: &[usize], start: usize, budget: usize) -> usize {
    let mut used = 0usize;
    let mut taken = 0;
    for &size in sizes.get(start..).unwrap_or_default() {
        if taken > 0 && used.saturating_add(size) > budget {
            break;
        }
        used = used.saturating_add(size);
        taken += 1;
    }
    taken
}

/// Distributes the item window `[start, end)` across consecutive sections, producing
/// one range per section (ranges may be empty). Followers: the cursor index space is
/// the flat concatenation of all sections, in the given order.
fn slice_ranges(section_lens: &[usize], start: usize, end: usize) -> Vec<Range<usize>> {
    let mut offset = 0;
    section_lens
        .iter()
        .map(|&len| {
            let range = start.saturating_sub(offset).min(len)..end.saturating_sub(offset).min(len);
            offset += len;
            range
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockAndReceiptsPageResponse {
    pub block: ExternalBlock,
    pub receipts: Vec<ExternalReceipt>,
    /// Present only in paginated streams; absent means the client received the
    /// complete block in a single response (one-shot / legacy leader).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pagination: Option<CursorPageInfo>,
}

/// Backward-compatible response: deserializes from both the paginated object
/// `{block, changes, pagination}` and the legacy `[block, changes]` tuple.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockWithChangesPageResponse {
    pub block: BlockRocksdb,
    pub changes: BlockChangesRocksdb,
    /// Present only in paginated streams; absent means the client received the
    /// complete block in a single response (one-shot / legacy leader).
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

pub struct BlockHashCursor {
    block_hash: Hash,
}

impl BlockHashCursor {
    pub(crate) fn new(block_hash: Hash) -> Self {
        Self { block_hash }
    }
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
    use itertools::Itertools;
    use jsonrpsee::types::Params;
    use serde_json::json;

    use super::BlockHashCursor;
    use super::CursorCodec;
    use super::IMPORTER_PAGE_LIMIT_DEFAULT;
    use super::IMPORTER_PAGE_LIMIT_MAX;
    use super::IMPORTER_PAGE_LIMIT_MIN;
    use super::ImporterPageRequest;
    use super::ImporterPagination;
    use super::PagePolicy;
    use super::RpcError;
    use super::json_len;
    use super::take_by_budget;
    use crate::alias::AlloyReceipt;
    use crate::alias::AlloyTransaction;
    use crate::eth::rpc::BlockFilter;
    use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
    use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
    use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
    use crate::eth::types::Block;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::ExternalBlockWithReceipts;
    use crate::eth::types::ExternalReceipt;
    use crate::eth::types::ExternalTransaction;
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
    fn limit_none_returns_none() {
        let req = ImporterPageRequest { cursor: None, limit: None };
        assert_eq!(req.explicit_limit(), None);
    }

    #[test]
    fn limit_zero_returns_default() {
        let req = ImporterPageRequest { cursor: None, limit: Some(0) };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_DEFAULT));
    }

    #[test]
    fn limit_small_value_passed_through() {
        let req = ImporterPageRequest { cursor: None, limit: Some(50) };
        assert_eq!(req.explicit_limit(), Some(50));
    }

    #[test]
    fn limit_below_floor_clamped_to_min() {
        let req = ImporterPageRequest { cursor: None, limit: Some(1) };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_MIN));
    }

    #[test]
    fn limit_large_value_clamped_to_max() {
        let req = ImporterPageRequest {
            cursor: None,
            limit: Some(10_000),
        };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_MAX));
    }

    // ImporterPagination::from_params

    const TEST_MAX_RESPONSE_BYTES: u32 = 20_000_000; // item budget = cap - envelope reserve

    #[test]
    fn from_params_no_param_uses_response_cap_minus_envelope_reserve() {
        let params = Params::new(Some("[]"));
        let (filter, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES).expect("ok");

        assert_eq!(pagination.start, 0);
        assert!(matches!(pagination.policy, PagePolicy::Bytes(budget) if budget == TEST_MAX_RESPONSE_BYTES as usize - ImporterPagination::ENVELOPE_RESERVE));
        assert!(matches!(filter, BlockFilter::Latest));
    }

    #[test]
    fn from_params_with_cursor_resolves_hash_filter_and_start() {
        let block_hash = "0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c";
        let cursor = codec(block_hash).encode_cursor(5);
        let json = format!(r#"[{{"cursor":"{cursor}","limit":10}}]"#);

        let params = Params::new(Some(&json));
        let (filter, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES).expect("ok");

        assert!(matches!(filter, BlockFilter::Hash(h) if h == block_hash.parse::<Hash>().unwrap()));
        assert_eq!(pagination.start, 5);
        // limit 10 is below the floor and clamps up to IMPORTER_PAGE_LIMIT_MIN
        assert!(matches!(pagination.policy, PagePolicy::Count(IMPORTER_PAGE_LIMIT_MIN)));
    }

    #[test]
    fn from_params_null_cursor_and_no_limit_uses_byte_budget() {
        let params = Params::new(Some(r#"[{"cursor":null}]"#));
        let (_filter, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES).expect("ok");

        assert_eq!(pagination.start, 0);
        assert!(matches!(pagination.policy, PagePolicy::Bytes(_)));
    }

    // block_and_receipts_response (server slicing)

    fn block_with_txs(count: usize) -> Block {
        let mut block = Block::new(BlockNumber::from(1u64), UnixTime::from(0u64));
        block.transactions = std::iter::repeat_with(|| Faker.fake()).take(count).collect();
        block
    }

    #[test]
    fn block_and_receipts_single_page_returns_legacy_one_shot() {
        // total (3) <= limit (10): complete block in one response, no pagination field.
        let block = block_with_txs(3);
        let pagination = ImporterPagination::for_test(0, 10);
        let json = pagination.block_and_receipts_response(block).expect("ok");

        assert!(json.get("pagination").is_none());
        let one_shot: ExternalBlockWithReceipts = serde_json::from_value(json).expect("legacy shape");
        assert_eq!(one_shot.receipts.len(), 3);
    }

    #[test]
    fn block_and_receipts_multi_page_slices_correctly() {
        let block = block_with_txs(5);

        // page 1: start=0, limit=3
        let pagination = ImporterPagination::for_test(0, 3);
        let json = pagination.block_and_receipts_response(block.clone()).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
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
        let json = pagination.block_and_receipts_response(block).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
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
        let json = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
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
        let json = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
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
        let json = pagination.block_with_changes_response(block, changes).expect("ok");
        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 0);
        assert_eq!(response.changes.slot_changes.len(), 1);
        assert!(info.next_cursor.is_none());
    }

    #[test]
    fn block_with_changes_single_page_returns_legacy_one_shot() {
        // total (6) <= limit (10): legacy tuple shape, no pagination field.
        let block = block_rocksdb_with_txs(1);
        let changes = changes_fixture();

        let pagination = ImporterPagination::for_test(0, 10);
        let json = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");

        assert!(json.get("pagination").is_none());
        let (one_shot_block, one_shot_changes): (BlockRocksdb, BlockChangesRocksdb) = serde_json::from_value(json).expect("legacy tuple shape");
        assert_eq!(one_shot_block, block);
        assert_eq!(one_shot_changes, changes);
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
        let pagination = ImporterPagination::for_test(0, 1);
        let json = pagination.block_and_receipts_response(block).expect("ok");
        assert!(json.get("pagination").is_some());

        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated response deserializes");
        assert!(response.pagination.is_some());
    }

    #[test]
    fn receipts_response_omits_pagination_key_when_serializing_none() {
        let response = BlockAndReceiptsPageResponse {
            block: Faker.fake(),
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
        let pagination = ImporterPagination::for_test(0, 1);
        let json = pagination.block_with_changes_response(block, changes).expect("ok");
        assert!(json.get("pagination").is_some());

        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated response deserializes");
        assert!(response.pagination.is_some());
    }

    // Byte-budget policy (production default)

    // every fixture item serializes to more than 1 byte, so budget=1 packs exactly
    // one item per page — a deterministic boundary for byte-budget behavior tests.
    const BYTE_BUDGET_ONE_ITEM: usize = 1;

    #[test]
    fn receipts_byte_budget_max_returns_one_shot() {
        let block = block_with_txs(5);
        let json = ImporterPagination::for_budget_test(0, usize::MAX)
            .block_and_receipts_response(block)
            .expect("ok");
        assert!(json.get("pagination").is_none());
    }

    #[test]
    fn receipts_byte_budget_packs_items_to_byte_boundary() {
        let block = block_with_txs(5);
        let pagination = ImporterPagination::for_budget_test(0, BYTE_BUDGET_ONE_ITEM);
        let json = pagination.block_and_receipts_response(block).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        // exactly one item per page: 5 pages of 1 tx each
        assert_eq!(info.returned, 1);
        assert_eq!(info.total, 5);
        assert_eq!(info.limit, BYTE_BUDGET_ONE_ITEM);
        assert_eq!(response.receipts.len(), 1);
        let (_, next) = BlockHashCursor::decode_cursor(&info.next_cursor.expect("more pages")).expect("valid cursor");
        assert_eq!(next, 1);
    }

    #[test]
    fn receipts_byte_budget_above_actual_response_is_one_shot_and_below_paginates() {
        let block = block_with_txs(3);

        // measure the actual one-shot response space: header + JSON structure
        // (frame) + per-item wire sizes and separator commas
        let empty_block: crate::alias::AlloyBlockAlloyTransaction = block.header.clone().into();
        let frame: usize = json_len(&json!({
            "block": empty_block.map_transactions(ExternalTransaction::from),
            "receipts": Vec::<ExternalReceipt>::new(),
        }));
        let receipts: Vec<ExternalReceipt> = block.transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect();
        let total_bytes: usize = frame
            + block
                .transactions
                .iter()
                .cloned()
                .map_into()
                .zip(&receipts)
                .map(|(tx, receipt): (AlloyTransaction, &ExternalReceipt)| json_len(&tx) + json_len(receipt) + 2)
                .sum::<usize>();

        let json = ImporterPagination::for_budget_test(0, total_bytes)
            .block_and_receipts_response(block.clone())
            .expect("ok");
        assert!(json.get("pagination").is_none(), "equal-to-emitted budget must one-shot");

        let json = ImporterPagination::for_budget_test(0, total_bytes.saturating_sub(1))
            .block_and_receipts_response(block)
            .expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");
        assert_eq!(info.total, 3);
        assert!(info.returned < 3);
        assert!(info.next_cursor.is_some());
    }

    #[test]
    fn receipts_byte_budget_single_oversized_item_still_emits() {
        // a single item larger than the budget must not stall the stream.
        let block = block_with_txs(1);
        let pagination = ImporterPagination::for_budget_test(0, BYTE_BUDGET_ONE_ITEM);
        let json = pagination.block_and_receipts_response(block).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        assert_eq!(info.total, 1);
        assert!(info.next_cursor.is_none());
    }

    #[test]
    fn take_by_budget_every_window_fits_unless_single_item() {
        // reference implementation: greedy window pack, always at least one item.
        fn expected_taken(sizes: &[usize], start: usize, budget: usize) -> usize {
            let mut used = 0usize;
            let mut taken = 0;
            for &size in &sizes[start..] {
                let new_used = used.saturating_add(size);
                if taken > 0 && new_used > budget {
                    break;
                }
                used = new_used;
                taken += 1;
            }
            taken
        }

        let budget = 10_000usize;
        let sizes: Vec<usize> = std::iter::repeat_with(|| (0..20_000usize).fake()).take(500).collect();

        let mut start = 0;
        while start < sizes.len() {
            let taken = take_by_budget(&sizes, start, budget);
            assert_eq!(taken, expected_taken(&sizes, start, budget));
            assert!(taken >= 1, "stream must always make progress");

            let window_sum: usize = sizes[start..start + taken].iter().copied().fold(0, usize::saturating_add);
            assert!(
                window_sum <= budget || taken == 1,
                "window of {taken} items at {start} sums {window_sum} bytes, over budget {budget}"
            );
            start += taken;
        }
        assert_eq!(start, sizes.len(), "stream must cover every item");
    }

    #[test]
    fn receipts_byte_budget_serialized_page_including_frame_fits_cap() {
        // walk a full paginated stream and measure the entire emitted response —
        // JSON structure, block header, items, commas, pagination metadata — against
        // the budget. The only legal overshoot is a single item whose payload alone
        // exceeds the item budget (guaranteed progress).
        let block = block_with_txs(30);
        let budget = serde_json::to_vec(&block.transactions[0]).unwrap().len()
            + serde_json::to_vec(&ExternalReceipt(AlloyReceipt::from(block.transactions[0].clone())))
                .unwrap()
                .len()
            + 2
            + 512; // room for a couple of items per page

        let mut start = 0;
        let mut total = usize::MAX;
        while start < total {
            let json = ImporterPagination::for_budget_test(start, budget)
                .block_and_receipts_response(block.clone())
                .expect("ok");
            let emitted_len = serde_json::to_vec(&json).unwrap().len();
            let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
            let info = response.pagination.expect("pagination present");
            total = info.total;

            assert!(
                emitted_len <= budget || info.returned == 1,
                "page starting at {start} emitted {emitted_len} bytes ({} items), over budget {budget}",
                info.returned
            );
            start += info.returned;
        }
        assert_eq!(start, 30, "whole block streamed");
    }

    #[test]
    fn receipts_byte_budget_continuation_uses_cursor_index() {
        let block = block_with_txs(5);
        let pagination = ImporterPagination::for_budget_test(2, BYTE_BUDGET_ONE_ITEM);
        let json = pagination.block_and_receipts_response(block).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        let (_, next) = BlockHashCursor::decode_cursor(&info.next_cursor.expect("more pages")).expect("valid cursor");
        assert_eq!(next, 3);
    }

    #[test]
    fn changes_byte_budget_packs_across_sections() {
        let block = block_rocksdb_with_txs(2);
        let changes = changes_fixture();
        // total = 2 txs + 3 accounts + 2 slots = 7

        // page 1: 2 txs + 1 account; page 2: 1 account; ...
        let pagination = ImporterPagination::for_budget_test(0, BYTE_BUDGET_ONE_ITEM);
        let json = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        assert_eq!(info.total, 7);
        // first page takes only the first tx
        assert_eq!(response.block.transactions.len(), 1);
        assert_eq!(response.changes.account_changes.len(), 0);

        // from cursor 3: items 3..7 = 4 (account 2, account 3, slot 1, slot 2)
        let pagination = ImporterPagination::for_budget_test(3, BYTE_BUDGET_ONE_ITEM);
        let json = pagination.block_with_changes_response(block, changes).expect("ok");
        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, 1);
        assert_eq!(response.block.transactions.len(), 0);
        assert_eq!(response.changes.account_changes.len(), 1);
        assert_eq!(response.changes.slot_changes.len(), 0);
        let (_, next) = BlockHashCursor::decode_cursor(&info.next_cursor.expect("more pages")).expect("valid cursor");
        assert_eq!(next, 4);
    }

    #[test]
    fn stale_cursor_beyond_total_errors() {
        let block = block_with_txs(3);
        assert!(matches!(
            ImporterPagination::for_test(3, 10).block_and_receipts_response(block.clone()),
            Err(RpcError::ParameterInvalid)
        ));
        assert!(matches!(
            ImporterPagination::for_test(4, 10).block_and_receipts_response(block.clone()),
            Err(RpcError::ParameterInvalid)
        ));
        assert!(matches!(
            ImporterPagination::for_budget_test(3, 100).block_and_receipts_response(block),
            Err(RpcError::ParameterInvalid)
        ));
    }

    #[test]
    fn empty_block_is_valid_one_shot() {
        let block = block_with_txs(0);
        let json = ImporterPagination::for_test(0, 10).block_and_receipts_response(block).expect("ok");
        assert!(json.get("pagination").is_none());
    }
}
