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

/// Default page size for the dev-only explicit `limit` override.
#[cfg(any(test, feature = "dev"))]
pub const IMPORTER_PAGE_LIMIT_DEFAULT: usize = 256;
/// Floor for an explicit `limit`: prevents pathological page counts (e.g. `limit: 1`
/// against a big block would emit thousands of pages).
#[cfg(any(test, feature = "dev"))]
pub const IMPORTER_PAGE_LIMIT_MIN: usize = 32;
#[cfg(any(test, feature = "dev"))]
pub const IMPORTER_PAGE_LIMIT_MAX: usize = 5_000;

const IMPORTER_CURSOR_VERSION: &str = "v1";

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImporterPageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Opt-in flag for byte-budget cursor pagination.
    ///
    /// Backward compatible: legacy clients (e.g. followers built before
    /// pagination) never send this field, so the leader answers with the complete
    /// one-shot response exactly as before this change — no behavior change for
    /// them. New clients opt in to receive paginated responses for blocks that
    /// exceed the server response cap. Pagination is never applied to a request
    /// that did not opt in, so a client that does not understand the paginated
    /// shape is never surprised by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pagination: Option<bool>,

    /// Item-count override that forces paginated, count-based responses.
    ///
    /// Not a production control: the production follower opts in without a `limit`
    /// and lets the leader page by response size. This exists so tests and tooling
    /// can exercise multi-page flows cheaply (hundreds of items instead of multi-MB
    /// blocks). Sending a `limit` implies pagination opt-in. Values are clamped to
    /// [`IMPORTER_PAGE_LIMIT_MIN`]..=[`IMPORTER_PAGE_LIMIT_MAX`].
    ///
    /// Only honored by servers built with the `dev` feature (or in tests):
    /// production builds strip the field.
    #[cfg(any(test, feature = "dev"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl ImporterPageRequest {
    /// Builds a first-page request opting in to byte-budget cursor pagination.
    pub fn opt_in() -> Self {
        Self {
            cursor: None,
            pagination: Some(true),
            #[cfg(any(test, feature = "dev"))]
            limit: None,
        }
    }

    /// Builds a continuation request carrying only the cursor. The byte-budget
    /// policy applies unless the original stream was count-based and the client
    /// resends its `limit`.
    pub fn with_cursor(cursor: String) -> Self {
        Self {
            cursor: Some(cursor),
            pagination: None,
            #[cfg(any(test, feature = "dev"))]
            limit: None,
        }
    }

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
    #[cfg(any(test, feature = "dev"))]
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
    /// Pagination is disabled for this request: the complete payload is always
    /// emitted as a single legacy response, byte-identical to the pre-pagination
    /// behavior.
    ///
    /// Used for legacy requests (clients that did not opt in to pagination) and when
    /// pagination is disabled server-side, so mixed-version deployments keep the
    /// exact behavior they had before this change.
    Legacy,
    /// Page by item count (explicit `limit` override — a test/tooling knob, not a
    /// production control; see [`ImporterPageRequest::limit`]). Sending a `limit`
    /// implies pagination opt-in.
    #[cfg(any(test, feature = "dev"))]
    Count(usize),
    /// Page by a serialized-bytes budget (production default for opted-in
    /// requests). Measured on the exact encoding each response endpoint emits, so a
    /// page is guaranteed to fit the server response cap.
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
    /// Pagination is opt-in and backward compatible. Policy resolution for a
    /// `[filter, page_request]` call:
    /// - an explicit `limit` forces count-based pagination (test/tooling knob);
    /// - `pagination: true` opts in to byte-budget pagination (sent by the importer
    ///   client);
    /// - a `cursor` resumes a paginated stream (byte-budget continuation);
    /// - anything else is a legacy request: the response is always the complete
    ///   one-shot payload, byte-identical to the pre-pagination behavior. Clients
    ///   that do not understand pagination (e.g. older followers) are unaffected —
    ///   they send the same request they always did and receive the same response.
    ///   For blocks that exceed the response cap, legacy clients keep the
    ///   pre-existing limitation; opting in lets new clients stream such blocks.
    ///
    /// When `pagination_enabled` is false (server-side kill switch), every request
    /// resolves to the legacy one-shot behavior, cursors included.
    ///
    /// Returns `(filter, pagination)` where `filter` is:
    /// - The original `filter` if no cursor is present (first page).
    /// - `BlockFilter::Hash(cursor.block_hash)` if a cursor is present (subsequent
    ///   pages), overriding the original filter to ensure the same block is
    ///   paginated.
    pub fn from_params(
        params: ParamsSequence<'_>,
        filter: BlockFilter,
        max_response_bytes: u32,
        pagination_enabled: bool,
    ) -> Result<(BlockFilter, Self), RpcError> {
        let request = ImporterPageRequest::parse_optional(params)?;
        let (filter, start) = Self::resolve_filter(filter, request.as_ref().and_then(|r| r.cursor.as_deref()))?;

        let opted_in = matches!(request.as_ref().and_then(|r| r.pagination), Some(true));
        let has_cursor = request.as_ref().is_some_and(|r| r.cursor.is_some());
        let policy = match (pagination_enabled, opted_in || has_cursor) {
            (false, _) | (true, false) => PagePolicy::Legacy,
            (true, true) => PagePolicy::Bytes((max_response_bytes as usize).saturating_sub(Self::ENVELOPE_RESERVE)),
        };
        // Dev-only `limit` override takes precedence and switches to count-based
        // paging. Honored only when pagination is enabled.
        #[cfg(any(test, feature = "dev"))]
        let policy = if !pagination_enabled {
            policy
        } else {
            match request.as_ref().and_then(ImporterPageRequest::explicit_limit) {
                Some(limit) => PagePolicy::Count(limit),
                None => policy,
            }
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
    /// Returns `(response, paginated)`. Paginates only when the request opted in and
    /// the payload does not fit entirely in one response (a continuation always
    /// paginates). Otherwise returns the legacy one-shot `{block, receipts}` shape
    /// with no `pagination` field.
    pub fn block_and_receipts_response(&self, block: Block) -> Result<(JsonValue, bool), RpcError> {
        let Block { header, transactions } = block;
        let total = transactions.len();
        self.validate_start(total)?;
        let block_hash = header.hash;

        let receipts: Vec<ExternalReceipt> = transactions.iter().cloned().map(AlloyReceipt::from).map(ExternalReceipt).collect();
        let mut transactions: Vec<AlloyTransaction> = transactions.into_iter().map_into().collect();

        let empty_block: AlloyBlockAlloyTransaction = header.clone().into();
        let base_frame = json_len(&json!({
            "block": empty_block.map_transactions(ExternalTransaction::from),
            "receipts": Vec::<ExternalReceipt>::new()
        }));

        let item_bytes = |idx: usize| json_len(&transactions[idx]).saturating_add(json_len(&receipts[idx])).saturating_add(2);
        let total_bytes = || (0..total).map(&item_bytes).fold(0usize, usize::saturating_add);
        let pack_from = |start: usize| take_by_budget(total, start, self.bytes_item_budget(base_frame), &item_bytes);

        let Some((end, limit_metric)) = self.page_window(base_frame, total, total_bytes, pack_from)? else {
            let mut block: AlloyBlockAlloyTransaction = header.into();
            block.transactions = BlockTransactions::Full(transactions);
            return Ok((
                json!({
                    "block": block.map_transactions(ExternalTransaction::from),
                    "receipts": receipts,
                }),
                false,
            ));
        };

        let tx_range = self.start..end;
        let page_transactions: Vec<AlloyTransaction> = transactions.drain(tx_range.clone()).collect();

        let mut block: AlloyBlockAlloyTransaction = header.into();
        block.transactions = BlockTransactions::Full(page_transactions);
        Ok((
            json!(BlockAndReceiptsPageResponse {
                block: ExternalBlock(block.map_transactions(ExternalTransaction::from)),
                receipts: receipts[tx_range].to_vec(),
                pagination: Some(self.page_info(end, limit_metric, total, block_hash)),
            }),
            true,
        ))
    }

    /// Builds the response for `stratus_getBlockWithChanges`.
    ///
    /// Same one-shot/paginate rule as [`Self::block_and_receipts_response`]; the
    /// one-shot shape is the legacy `[block, changes]` tuple, emitted without
    /// materializing (cloning/sorting) the changes map entries.
    pub fn block_with_changes_response(&self, block: BlockRocksdb, changes: BlockChangesRocksdb) -> Result<(JsonValue, bool), RpcError> {
        let BlockRocksdb { header, transactions } = block;
        let total = transactions.len() + changes.account_changes.len() + changes.slot_changes.len();
        self.validate_start(total)?;

        let section_lens = [transactions.len(), changes.account_changes.len(), changes.slot_changes.len()];

        // Frame: serialized `[block, changes]` tuple with empty item sections (block
        // header and JSON structural bytes).
        let base_frame = json_len(&json!((
            BlockRocksdb {
                header: header.clone(),
                transactions: Vec::new()
            },
            BlockChangesRocksdb::default()
        )));

        // Sorted entries materialize (clone every change) only when the request
        // paginates: the cursor index space is the flat concatenation of
        // transactions, address-sorted account changes and key-sorted slot
        // changes, so the sort is required to pack and slice pages — but never
        // for a one-shot.
        let mut sorted: Option<(Vec<_>, Vec<_>)> = None;
        let total_bytes = || {
            let tx_bytes = transactions.iter().map(|tx| json_len(tx).saturating_add(1)).fold(0usize, usize::saturating_add);
            let account_bytes = changes
                .account_changes
                .iter()
                .map(|(address, change)| json_len(&(*address, change)).saturating_add(1))
                .fold(0usize, usize::saturating_add);
            let slot_bytes = changes
                .slot_changes
                .iter()
                .map(|(key, value)| json_len(&(key, value)).saturating_add(1))
                .fold(0usize, usize::saturating_add);
            tx_bytes.saturating_add(account_bytes).saturating_add(slot_bytes)
        };
        let pack_from = |start: usize| {
            let (account_entries, slot_entries) = sorted.get_or_insert_with(|| (sorted_account_changes(&changes), sorted_slot_changes(&changes)));
            take_by_budget(total, start, self.bytes_item_budget(base_frame), |flat_idx| {
                flat_item_bytes(flat_idx, &transactions, account_entries, slot_entries)
            })
        };

        let Some((end, limit_metric)) = self.page_window(base_frame, total, total_bytes, pack_from)? else {
            return Ok((json!((BlockRocksdb { header, transactions }, changes)), false));
        };

        let ranges = slice_ranges(&section_lens, self.start, end);
        // Materialized here when the packer did not need it (count-based pages): the
        // cursor index space is defined by the sorted sections for any pagination
        // policy.
        let (account_entries, slot_entries) = sorted.get_or_insert_with(|| (sorted_account_changes(&changes), sorted_slot_changes(&changes)));

        let page_changes = BlockChangesRocksdb {
            account_changes: slice_account_changes(account_entries, ranges[1].clone()),
            slot_changes: slice_slot_changes(slot_entries, ranges[2].clone()),
        };

        Ok((
            json!(BlockWithChangesPageResponse {
                block: BlockRocksdb {
                    header: header.clone(),
                    transactions: transactions[ranges[0].clone()].to_vec(),
                },
                changes: page_changes,
                pagination: Some(self.page_info(end, limit_metric, total, header.hash.into())),
            }),
            true,
        ))
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
    /// Item payload budget of a byte-budget page: the cap minus the page frame
    /// (response structure plus pagination metadata). Only meaningful under
    /// [`PagePolicy::Bytes`]; other policies bound pages by item count instead.
    fn bytes_item_budget(&self, base_frame: usize) -> usize {
        match self.policy {
            PagePolicy::Bytes(cap) => cap.saturating_sub(base_frame.saturating_add(Self::PAGINATION_RESERVE)),
            _ => usize::MAX,
        }
    }

    /// Computes the page end index, or `None` when the response is a legacy one-shot.
    ///
    /// Continuations (`start > 0`) always paginate; first requests paginate only
    /// when the policy demands it (opted-in byte budget exceeded, or explicit
    /// `limit` surpassed).
    ///
    /// `total_bytes` measures the full item payload and is only evaluated for the
    /// byte-policy one-shot check. `pack_from(start)` computes the page end index
    /// by packing items from `start` and is only evaluated when the request
    /// paginates.
    fn page_window(
        &self,
        base_frame: usize,
        total: usize,
        total_bytes: impl FnOnce() -> usize,
        pack_from: impl FnOnce(usize) -> Result<usize, RpcError>,
    ) -> Result<Option<(usize, usize)>, RpcError> {
        // `total` is used only by the dev-only `Count` policy arm below.
        #[cfg(not(any(test, feature = "dev")))]
        let _ = total;
        match self.policy {
            PagePolicy::Legacy => Ok(None),
            #[cfg(any(test, feature = "dev"))]
            PagePolicy::Count(limit) =>
                if self.start > 0 || total > limit {
                    Ok(Some((self.start.saturating_add(limit).min(total), limit)))
                } else {
                    Ok(None)
                },
            PagePolicy::Bytes(cap) => {
                // One-shot when the whole payload fits the cap: item sizes and the
                // frame are measured on the exact emitted encoding, so the emitted
                // response (envelope included) is guaranteed to fit the server's max
                // response size.
                if self.start == 0 && total_bytes().saturating_add(base_frame) <= cap {
                    return Ok(None);
                }
                let budget = cap.saturating_sub(base_frame.saturating_add(Self::PAGINATION_RESERVE));
                let end = pack_from(self.start)?;
                Ok(Some((end, budget)))
            }
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
///
/// Note: the JSON-RPC transport may also aggregate responses (batch requests);
/// per-response budgets do not bound the aggregate body of a batch. The importer
/// issues single, non-batch requests.
fn json_len<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map(|json| json.len()).unwrap_or(usize::MAX)
}

/// Number of consecutive flat items starting at `start` that fit into `budget`,
/// sizing each item with `size_of`.
///
/// Always consumes at least one item; errors with
/// [`RpcError::PaginationItemTooLarge`] when even a single item exceeds `budget`,
/// since a page carrying it alone could never be delivered within the response cap —
/// failing loudly beats emitting a response the transport would refuse.
fn take_by_budget(total: usize, start: usize, budget: usize, mut size_of: impl FnMut(usize) -> usize) -> Result<usize, RpcError> {
    let mut used = 0usize;
    let mut taken = 0usize;
    for idx in start..total {
        let size = size_of(idx);
        if taken > 0 && used.saturating_add(size) > budget {
            break;
        }
        if taken == 0 && size > budget {
            return Err(RpcError::PaginationItemTooLarge { item_bytes: size, budget });
        }
        used = used.saturating_add(size);
        taken += 1;
    }
    Ok(start + taken)
}

/// Wire size of the flat item at `flat_idx` of the `block_with_changes` cursor
/// index space (transactions, then sorted account changes, then sorted slot
/// changes), charged for its separating comma.
///
/// Account and slot entries are sized in their tuple form, which is 2 bytes larger
/// than the emitted map entry (`"0x…":{…}`) — a conservative overestimate.
fn flat_item_bytes(
    flat_idx: usize,
    transactions: &[crate::eth::storage::permanent::rocks::types::TransactionMinedRocksdb],
    account_entries: &[(AddressRocksdb, AccountChangesRocksdb)],
    slot_entries: &[((AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb)],
) -> usize {
    let tx_len = transactions.len();
    if flat_idx < tx_len {
        json_len(&transactions[flat_idx]).saturating_add(1)
    } else if flat_idx < tx_len + account_entries.len() {
        json_len(&account_entries[flat_idx - tx_len]).saturating_add(1)
    } else {
        json_len(&slot_entries[flat_idx - tx_len - account_entries.len()]).saturating_add(1)
    }
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

    /// Wire size of the (tx, receipt) pair at `idx` as emitted (alloy encoding),
    /// charged for both separating commas.
    fn receipts_item_bytes(block: &Block, idx: usize) -> usize {
        let tx: AlloyTransaction = block.transactions[idx].clone().into();
        serde_json::to_vec(&tx).unwrap().len()
            + serde_json::to_vec(&ExternalReceipt(AlloyReceipt::from(block.transactions[idx].clone())))
                .unwrap()
                .len()
            + 2
    }

    /// Frame size of the emitted `stratus_getBlockAndReceipts` payload with an empty
    /// transaction list.
    fn receipts_frame(block: &Block) -> usize {
        let empty_block: crate::alias::AlloyBlockAlloyTransaction = block.header.clone().into();
        serde_json::to_vec(&json!({
            "block": empty_block.map_transactions(ExternalTransaction::from),
            "receipts": Vec::<ExternalReceipt>::new(),
        }))
        .unwrap()
        .len()
    }

    /// Byte budget whose item payload budget equals the largest (tx, receipt) pair:
    /// guarantees at least one item per page without oversized rejections.
    fn receipts_one_item_budget(block: &Block) -> usize {
        let max_item = (0..block.transactions.len()).map(|idx| receipts_item_bytes(block, idx)).max().unwrap_or(0);
        receipts_frame(block) + ImporterPagination::PAGINATION_RESERVE + max_item
    }

    /// Reference greedy pack: number of items taken from `start` under `budget`.
    fn expected_taken(sizes: &[usize], start: usize, budget: usize) -> usize {
        let mut used = 0usize;
        let mut taken = 0usize;
        for &size in &sizes[start..] {
            if taken > 0 && used + size > budget {
                break;
            }
            used += size;
            taken += 1;
        }
        taken
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
        let req = ImporterPageRequest {
            cursor: None,
            pagination: None,
            limit: None,
        };
        assert_eq!(req.explicit_limit(), None);
    }

    #[test]
    fn limit_zero_returns_default() {
        let req = ImporterPageRequest {
            cursor: None,
            pagination: None,
            limit: Some(0),
        };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_DEFAULT));
    }

    #[test]
    fn limit_small_value_passed_through() {
        let req = ImporterPageRequest {
            cursor: None,
            pagination: None,
            limit: Some(50),
        };
        assert_eq!(req.explicit_limit(), Some(50));
    }

    #[test]
    fn limit_below_floor_clamped_to_min() {
        let req = ImporterPageRequest {
            cursor: None,
            pagination: None,
            limit: Some(1),
        };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_MIN));
    }

    #[test]
    fn limit_large_value_clamped_to_max() {
        let req = ImporterPageRequest {
            cursor: None,
            pagination: None,
            limit: Some(10_000),
        };
        assert_eq!(req.explicit_limit(), Some(IMPORTER_PAGE_LIMIT_MAX));
    }

    // ImporterPagination::from_params

    const TEST_MAX_RESPONSE_BYTES: u32 = 20_000_000; // item budget = cap - envelope reserve

    #[test]
    fn from_params_plain_request_is_legacy_even_when_opt_in_available() {
        // Old clients send no second parameter: pagination must never apply.
        let params = Params::new(Some("[]"));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");
        assert_eq!(pagination.start, 0);
        assert_eq!(pagination.policy, PagePolicy::Legacy);
    }

    #[test]
    fn from_params_opt_in_uses_response_cap_minus_envelope_reserve() {
        let params = Params::new(Some(r#"[{"pagination": true}]"#));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");

        assert_eq!(pagination.start, 0);
        assert!(matches!(pagination.policy, PagePolicy::Bytes(budget) if budget == TEST_MAX_RESPONSE_BYTES as usize - ImporterPagination::ENVELOPE_RESERVE));
    }

    #[test]
    fn from_params_opt_in_false_is_legacy() {
        let params = Params::new(Some(r#"[{"pagination": false}]"#));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");
        assert_eq!(pagination.policy, PagePolicy::Legacy);
    }

    #[test]
    fn from_params_explicit_limit_forces_count_policy() {
        let json = r#"[{"pagination": true, "limit": 10}]"#;
        let params = Params::new(Some(json));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");

        // limit 10 is below the floor and clamps up to IMPORTER_PAGE_LIMIT_MIN
        assert!(matches!(pagination.policy, PagePolicy::Count(IMPORTER_PAGE_LIMIT_MIN)));
    }

    #[test]
    fn from_params_limit_alone_implies_opt_in() {
        let params = Params::new(Some(r#"[{"limit": 100}]"#));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");
        assert!(matches!(pagination.policy, PagePolicy::Count(100)));
    }

    #[test]
    fn from_params_cursor_continuation_paginates_by_budget() {
        let block_hash = "0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c";
        let cursor = codec(block_hash).encode_cursor(5);
        let json = format!(r#"[{{"cursor":"{cursor}"}}]"#);

        let params = Params::new(Some(&json));
        let (filter, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, true).expect("ok");

        assert!(matches!(filter, BlockFilter::Hash(h) if h == block_hash.parse::<Hash>().unwrap()));
        assert_eq!(pagination.start, 5);
        assert!(matches!(pagination.policy, PagePolicy::Bytes(_)));
    }

    #[test]
    fn from_params_disabled_switch_resolves_everything_to_legacy() {
        let block_hash = "0x3355a48e6b3e3a3c9e9c4b3a3f3e3d3c3b3a393837363534333231302f2e2d2c";
        let cursor = codec(block_hash).encode_cursor(5);
        let json = format!(r#"[{{"pagination": true, "cursor": "{cursor}", "limit": 10}}]"#);

        let params = Params::new(Some(&json));
        let (_, pagination) = ImporterPagination::from_params(params.sequence(), BlockFilter::Latest, TEST_MAX_RESPONSE_BYTES, false).expect("ok");
        assert_eq!(pagination.policy, PagePolicy::Legacy);
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
        let (json, _) = pagination.block_and_receipts_response(block).expect("ok");

        assert!(json.get("pagination").is_none());
        let one_shot: ExternalBlockWithReceipts = serde_json::from_value(json).expect("legacy shape");
        assert_eq!(one_shot.receipts.len(), 3);
    }

    #[test]
    fn block_and_receipts_multi_page_slices_correctly() {
        let block = block_with_txs(5);

        // page 1: start=0, limit=3
        let pagination = ImporterPagination::for_test(0, 3);
        let (json, _) = pagination.block_and_receipts_response(block.clone()).expect("ok");
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
        let (json, _) = pagination.block_and_receipts_response(block).expect("ok");
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
        let (json, _) = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
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
        let (json, _) = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");
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
        let (json, _) = pagination.block_with_changes_response(block, changes).expect("ok");
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
        let (json, _) = pagination.block_with_changes_response(block.clone(), changes.clone()).expect("ok");

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
        let (json, _) = pagination.block_and_receipts_response(block).expect("ok");
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
        let (json, _) = pagination.block_with_changes_response(block, changes).expect("ok");
        assert!(json.get("pagination").is_some());

        let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated response deserializes");
        assert!(response.pagination.is_some());
    }

    // Byte-budget policy (production default)

    #[test]
    fn receipts_byte_budget_max_returns_one_shot() {
        let block = block_with_txs(5);
        let (json, _) = ImporterPagination::for_budget_test(0, usize::MAX)
            .block_and_receipts_response(block)
            .expect("ok");
        assert!(json.get("pagination").is_none());
    }

    #[test]
    fn receipts_byte_budget_packs_items_to_byte_boundary() {
        let block = block_with_txs(5);
        let budget = receipts_one_item_budget(&block);
        let item_budget = budget - receipts_frame(&block) - ImporterPagination::PAGINATION_RESERVE;
        let sizes: Vec<usize> = (0..5).map(|idx| receipts_item_bytes(&block, idx)).collect();

        let mut start = 0;
        let mut pages = 0;
        while start < 5 {
            let (json, _) = ImporterPagination::for_budget_test(start, budget)
                .block_and_receipts_response(block.clone())
                .expect("ok");
            let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
            let info = response.pagination.expect("pagination present");

            // the reported limit is the exact item payload budget
            assert_eq!(info.limit, item_budget);
            // packing matches the reference greedy
            assert_eq!(info.returned, expected_taken(&sizes, start, item_budget));
            start += info.returned;
            pages += 1;
        }
        assert_eq!(start, 5, "whole block streamed");
        assert!(pages > 1, "small item budget must produce multiple pages");
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

        let (json, _) = ImporterPagination::for_budget_test(0, total_bytes)
            .block_and_receipts_response(block.clone())
            .expect("ok");
        assert!(json.get("pagination").is_none(), "equal-to-emitted budget must one-shot");

        let (json, _) = ImporterPagination::for_budget_test(0, total_bytes.saturating_sub(1))
            .block_and_receipts_response(block)
            .expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");
        assert_eq!(info.total, 3);
        assert!(info.returned < 3);
        assert!(info.next_cursor.is_some());
    }

    #[test]
    fn receipts_byte_budget_single_oversized_item_is_rejected() {
        // a single item larger than the budget cannot be delivered within the
        // response cap, so the server returns an explicit error instead of emitting
        // a page the transport would refuse.
        let block = block_with_txs(1);
        let budget = receipts_frame(&block) + ImporterPagination::PAGINATION_RESERVE + 1; // item budget = 1
        let result = ImporterPagination::for_budget_test(0, budget).block_and_receipts_response(block);
        assert!(matches!(result, Err(RpcError::PaginationItemTooLarge { .. })));
    }

    #[test]
    fn receipts_byte_budget_single_item_block_fits_when_whole_response_fits() {
        // one big item still one-shots when the whole response fits the cap.
        let block = block_with_txs(1);
        let item_bytes = serde_json::to_vec(&block.transactions[0]).unwrap().len();
        let budget = receipts_frame(&block) + ImporterPagination::PAGINATION_RESERVE + item_bytes * 4;
        let (json, paginated) = ImporterPagination::for_budget_test(0, budget).block_and_receipts_response(block).expect("ok");
        assert!(!paginated);
        assert!(json.get("pagination").is_none());
    }

    #[test]
    fn take_by_budget_every_window_fits_unless_single_item() {
        // reference behavior: greedy window pack; every window fits the budget and the
        // stream covers every item. The return value is the END index of the window.
        let budget = 10_000usize;
        let sizes: Vec<usize> = std::iter::repeat_with(|| (0..2_000usize).fake()).take(500).collect();
        let size_of = |idx: usize| sizes[idx];

        let mut start = 0;
        while start < sizes.len() {
            let end = take_by_budget(sizes.len(), start, budget, size_of).expect("no oversized items");
            assert!(end > start, "stream must always make progress");

            let window_sum: usize = sizes[start..end].iter().copied().fold(0, usize::saturating_add);
            assert!(
                window_sum <= budget,
                "window of {} items at {start} sums {window_sum} bytes, over budget {budget}",
                end - start
            );
            start = end;
        }
        assert_eq!(start, sizes.len(), "stream must cover every item");
    }

    #[test]
    fn take_by_budget_first_item_over_budget_errors() {
        let sizes = [50_000usize, 10, 10];
        let result = take_by_budget(sizes.len(), 0, 10_000, |idx| sizes[idx]);
        assert!(matches!(
            result,
            Err(RpcError::PaginationItemTooLarge {
                item_bytes: 50_000,
                budget: 10_000
            })
        ));

        // a later page starting on the oversized item errors the same way
        let sizes = [10usize, 50_000, 10];
        let result = take_by_budget(sizes.len(), 1, 10_000, |idx| sizes[idx]);
        assert!(matches!(result, Err(RpcError::PaginationItemTooLarge { .. })));
    }

    #[test]
    fn take_by_budget_stops_before_item_that_overflows_window() {
        // the greedy pack never takes an item that pushes the window over the budget
        // (only the mandatory first item may overflow, and that is an error instead).
        let sizes = [5_000usize, 9_999, 50_000];
        let end = take_by_budget(sizes.len(), 0, 10_000, |idx| sizes[idx]).expect("first item fits");
        assert_eq!(end, 1); // 5000 taken; 5000 + 9999 > 10000 stops the window
    }

    #[test]
    fn receipts_byte_budget_serialized_page_including_frame_fits_cap() {
        // walk a full paginated stream and measure the entire emitted response —
        // JSON structure, block header, items, commas, pagination metadata — against
        // the budget.
        let block = block_with_txs(30);
        let max_item = (0..30).map(|idx| receipts_item_bytes(&block, idx)).max().unwrap();
        // item budget = largest item + slack -> a couple of items per page, no
        // oversized rejections
        let budget = receipts_frame(&block) + ImporterPagination::PAGINATION_RESERVE + max_item + 512;
        let item_budget = budget - receipts_frame(&block) - ImporterPagination::PAGINATION_RESERVE;
        let sizes: Vec<usize> = (0..30).map(|idx| receipts_item_bytes(&block, idx)).collect();

        let mut start = 0;
        while start < 30 {
            let (json, _) = ImporterPagination::for_budget_test(start, budget)
                .block_and_receipts_response(block.clone())
                .expect("ok");
            let emitted_len = serde_json::to_vec(&json).unwrap().len();
            let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
            let info = response.pagination.expect("pagination present");

            assert_eq!(info.returned, expected_taken(&sizes, start, item_budget));
            assert!(
                emitted_len <= budget,
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
        let budget = receipts_one_item_budget(&block);
        let item_budget = budget - receipts_frame(&block) - ImporterPagination::PAGINATION_RESERVE;
        let sizes: Vec<usize> = (0..5).map(|idx| receipts_item_bytes(&block, idx)).collect();

        let (json, _) = ImporterPagination::for_budget_test(2, budget).block_and_receipts_response(block).expect("ok");
        let response: BlockAndReceiptsPageResponse = serde_json::from_value(json).expect("paginated shape");
        let info = response.pagination.expect("pagination present");

        assert_eq!(info.returned, expected_taken(&sizes, 2, item_budget));
        let (_, next) = BlockHashCursor::decode_cursor(&info.next_cursor.expect("more pages")).expect("valid cursor");
        assert_eq!(next, 2 + info.returned);
    }

    #[test]
    fn changes_byte_budget_packs_across_sections() {
        let block = block_rocksdb_with_txs(2);
        let changes = changes_fixture();
        // total = 2 txs + 3 accounts + 2 slots = 7

        // flat item sizes in cursor index space (txs, sorted accounts, sorted slots)
        let account_entries = super::sorted_account_changes(&changes);
        let slot_entries = super::sorted_slot_changes(&changes);
        let mut sizes: Vec<usize> = block.transactions.iter().map(|tx| serde_json::to_vec(tx).unwrap().len() + 1).collect();
        sizes.extend(account_entries.iter().map(|entry| serde_json::to_vec(entry).unwrap().len() + 1));
        sizes.extend(slot_entries.iter().map(|entry| serde_json::to_vec(entry).unwrap().len() + 1));

        // frame: [block, changes] tuple with empty item sections
        let frame = {
            let empty_block = BlockRocksdb {
                header: block.header.clone(),
                transactions: Vec::new(),
            };
            serde_json::to_vec(&json!((empty_block, BlockChangesRocksdb::default()))).unwrap().len()
        };
        let max_item = sizes.iter().copied().max().unwrap();
        let budget = frame + ImporterPagination::PAGINATION_RESERVE + max_item;
        let item_budget = budget - frame - ImporterPagination::PAGINATION_RESERVE;

        let mut start = 0;
        while start < sizes.len() {
            let (json, _) = ImporterPagination::for_budget_test(start, budget)
                .block_with_changes_response(block.clone(), changes.clone())
                .expect("ok");
            let response: BlockWithChangesPageResponse = serde_json::from_value(json).expect("paginated shape");
            let info = response.pagination.expect("pagination present");

            assert_eq!(info.limit, item_budget);
            assert_eq!(info.returned, expected_taken(&sizes, start, item_budget));
            start += info.returned;
        }
        assert_eq!(start, sizes.len(), "whole stream covered");
    }

    #[test]
    fn changes_byte_budget_single_oversized_item_is_rejected() {
        let block = block_rocksdb_with_txs(1);
        let changes = changes_fixture();
        let frame = {
            let empty_block = BlockRocksdb {
                header: block.header.clone(),
                transactions: Vec::new(),
            };
            serde_json::to_vec(&json!((empty_block, BlockChangesRocksdb::default()))).unwrap().len()
        };
        let budget = frame + ImporterPagination::PAGINATION_RESERVE + 1; // item budget = 1
        let result = ImporterPagination::for_budget_test(0, budget).block_with_changes_response(block, changes);
        assert!(matches!(result, Err(RpcError::PaginationItemTooLarge { .. })));
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
        let (json, _) = ImporterPagination::for_test(0, 10).block_and_receipts_response(block).expect("ok");
        assert!(json.get("pagination").is_none());
    }
}
