use jsonrpsee::types::ParamsSequence;

use super::BlockFilter;
use super::RpcError;
use super::pagination::CursorCodec;
use super::pagination::CursorPageInfo;
use super::pagination::CursorPaginationPolicy;
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

    fn limit(&self) -> usize {
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
    pub fn next(params: ParamsSequence<'_>, filter: BlockFilter) -> Result<Option<(BlockFilter, Self)>, RpcError> {
        let Some(request) = ImporterPageRequest::parse_next(params)? else {
            return Ok(None);
        };
        let (filter, start) = Self::resolve_filter(filter, request.cursor.as_deref())?;
        Ok(Some((filter, Self { request, start })))
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
        ImporterCursorPaginator::new(
            total,
            self.start,
            self.request.limit(),
            CursorPaginationPolicy::new(BlockHashCursor { block_hash }),
        )
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

pub(crate) type ImporterCursorPaginator = CursorPaginator<BlockHashCursor>;

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
