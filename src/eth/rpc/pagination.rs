//! Response pagination for oversized importer RPC responses.
//!
//! The importer endpoints (`stratus_getBlockAndReceipts` and `stratus_getBlockWithChanges`) can
//! produce responses larger than [`MAX_RESPONSE_SIZE_BYTES`] (leader), which makes the importer
//! get stuck retrying forever. This module implements a minimal, stateless pagination protocol
//! to circumvent that:
//!
//! - The follower signals pagination support by sending an optional second parameter object
//!   [`PaginationParams`] alongside the block filter. Old leaders ignore extra params, so the
//!   request degrades to today's behavior.
//! - When the serialized response does not fit in one message, the leader serializes it once and
//!   answers with [`PaginatedResponse`], an envelope carrying one fragment (`chunk`) of the
//!   serialized response encoded as base64. The follower keeps requesting further offsets and
//!   reassembles the decoded fragments until it has the full serialized response, which it then
//!   deserializes normally.
//!
//! Wire format (leader result when paginating):
//!
//! ```json
//! {"stratus_paginated": {"total": 12345, "chunk": "eyJibG9jayI6..."}}
//! ```
//!
//! The `chunk` string is the base64 of a raw slice of the serialized response. Base64 is pure
//! ASCII and needs no JSON escaping, so the chunk size is plain arithmetic (3 response bytes per
//! 4 wire characters), and slices may cut anywhere in the response — including in the middle of a
//! multi-byte UTF-8 character — since the follower reassembles opaque bytes and parses the JSON
//! only once at the end. The envelope has exactly one top-level key, which always serializes
//! first, allowing O(1) prefix detection ([`is_envelope`]) on the follower side without parsing the
//! (potentially huge) response.

use anyhow::Context;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use jsonrpsee::types::ParamsSequence;
use serde::Deserialize;
use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::value::to_raw_value;

use crate::alias::JsonValue;
use crate::eth::rpc::RpcError;
use crate::eth::types::StratusError;
use crate::ext::to_json_value;

/// Wire prefix of a paginated envelope result.
///
/// Must stay in sync with [`PaginatedResponse`]'s single field name: since the envelope has
/// exactly one top-level key, it always serializes first, which allows O(1) prefix detection in
/// [`is_envelope`] without parsing the response. Pinned by `envelope_wire_format_is_stable`.
const ENVELOPE_PREFIX: &str = "{\"stratus_paginated\":";

/// Bytes reserved for the JSON-RPC response envelope (`{"jsonrpc":"2.0","id":..,"result":..}`)
/// and the pagination envelope object itself, on top of the base64 chunk.
pub(crate) const MARGIN: u32 = 512;

/// Minimum chunk budget worth advertising; used in the floor below.
const MIN_CHUNK_BUDGET: u32 = 64;

/// Minimum response size limit for pagination to make progress: the envelope margin, one minimum
/// chunk, and headroom for the envelope and JSON-RPC wrappers.
///
/// Enforced at startup by the `MAX_RESPONSE_SIZE_BYTES` clap argument.
pub const MIN_RESPONSE_SIZE_BYTES: u32 = MARGIN + MIN_CHUNK_BUDGET + 128;

/// Maximum bytes preallocated for reassembly, to avoid OOM on a bogus `total` from a malicious peer.
const MAX_REASSEMBLY_PREALLOC: usize = 1024 * 1024;

/// Hard cap on the total size of a reassembled paginated response, as defense against a
/// malicious or buggy leader advertising an arbitrarily large `total`.
pub const MAX_REASSEMBLY_TOTAL: u64 = 512 * 1024 * 1024;

/// Optional second parameter sent by pagination-capable followers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    /// Byte offset of the requested chunk within the serialized response.
    pub offset: u64,
}

/// Extracts the optional [`PaginationParams`] from the remaining request params sequence.
pub fn parse_request(mut params: ParamsSequence<'_>) -> Result<Option<PaginationParams>, RpcError> {
    params.optional_next::<PaginationParams>().map_err(|e| RpcError::ParameterDecodeError {
        rust_type: "PaginationParams",
        decode_error: e.data().map(|d| d.to_string()).unwrap_or_default(),
    })
}

/// One chunk of the serialized response, plus its total size in bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationEnvelope {
    /// Total size in bytes of the fully serialized response.
    pub total: u64,

    /// Base64 of the raw fragment of the serialized response, starting at the requested offset.
    pub chunk: String,
}

/// Leader result when the response does not fit in a single message.
///
/// The sentinel field name doubles as the JSON key; having a single top-level field guarantees it
/// serializes first, which [`is_envelope`] relies on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse {
    pub stratus_paginated: PaginationEnvelope,
}

/// Returns whether a raw result is a paginated envelope (O(1), no parsing).
pub fn is_envelope(raw: &str) -> bool {
    raw.starts_with(ENVELOPE_PREFIX)
}

/// Parses a raw result assumed to be a paginated envelope.
pub fn parse_envelope(raw: &str) -> anyhow::Result<PaginationEnvelope> {
    let response = serde_json::from_str::<PaginatedResponse>(raw).map_err(|e| anyhow::anyhow!(e).context("failed to parse paginated response envelope"))?;
    Ok(response.stratus_paginated)
}

/// Builds the method result, paginating when the follower asked for it and the response is too big.
///
/// - `pagination` absent: returns the value as-is (byte-identical to the non-paginated behavior).
/// - `pagination` present and the serialized value fits in one response: returns the full value.
/// - `pagination` present and the serialized value does not fit: returns a [`PaginatedResponse`]
///   with the chunk starting at the requested offset.
///
/// The leader alone decides the chunk size, derived from its own response size limit.
pub fn respond(value: JsonValue, pagination: Option<PaginationParams>, max_response_bytes: u32) -> Result<Box<RawValue>, StratusError> {
    let raw = to_raw_value(&value).map_err(|e| StratusError::Unexpected(crate::eth::types::UnexpectedError::Unexpected(anyhow::anyhow!(e))))?;

    let Some(pagination) = pagination else {
        return Ok(raw);
    };

    let full = raw.get();
    let budget = max_response_bytes.saturating_sub(MARGIN) as usize;

    if full.len() <= budget {
        return Ok(raw);
    }

    let offset = usize::try_from(pagination.offset).unwrap_or(usize::MAX);
    if offset > full.len() {
        tracing::error!(offset, total = full.len(), "pagination offset is beyond the response size");
        return Err(RpcError::ParameterInvalid.into());
    }

    // the budget bounds the encoded chunk on the wire, but the slice is in raw response bytes:
    // base64 turns 3 raw bytes into 4 wire characters, so 3/4 of the budget is the largest slice
    // that still fits; dividing by 4 first makes the encoded length land exactly on the budget
    let raw_budget = budget / 4 * 3;
    let end = offset.saturating_add(raw_budget).min(full.len());
    let chunk = BASE64_STANDARD.encode(&full.as_bytes()[offset..end]);
    tracing::debug!(
        total = full.len(),
        offset,
        chunk_bytes = end - offset,
        budget,
        "paginating oversized rpc response"
    );

    let envelope = PaginatedResponse {
        stratus_paginated: PaginationEnvelope {
            total: full.len() as u64,
            chunk,
        },
    };
    to_raw_value(&envelope).map_err(|e| StratusError::Unexpected(crate::eth::types::UnexpectedError::Unexpected(anyhow::anyhow!(e))))
}

/// Builds pagination request params for the follower side.
pub fn request_params(offset: u64) -> JsonValue {
    to_json_value(PaginationParams { offset })
}

/// Progressive reassembly of a paginated response, with validation against a malicious peer.
#[derive(Debug)]
pub struct Reassembler {
    total: u64,
    received: Vec<u8>,
}

impl Reassembler {
    /// Creates a reassembler for a response of `total` bytes.
    pub fn new(total: u64) -> Self {
        let capacity = usize::try_from(total).unwrap_or(usize::MAX).min(MAX_REASSEMBLY_PREALLOC);
        Self {
            total,
            received: Vec::with_capacity(capacity),
        }
    }

    /// Appends a chunk and returns whether the response is complete.
    pub fn push(&mut self, envelope: PaginationEnvelope) -> anyhow::Result<bool> {
        if envelope.total != self.total {
            anyhow::bail!("pagination total changed mid-response: expected {}, got {}", self.total, envelope.total);
        }

        let chunk = BASE64_STANDARD.decode(&envelope.chunk).context("pagination chunk is not valid base64")?;
        let done = self.received.len() as u64 + chunk.len() as u64 >= self.total;
        if !done && chunk.is_empty() {
            anyhow::bail!("received empty pagination chunk with {} of {} bytes", self.received.len(), self.total);
        }

        self.received.extend_from_slice(&chunk);
        Ok(done)
    }

    /// Returns the next byte offset to request.
    pub fn next_offset(&self) -> u64 {
        self.received.len() as u64
    }

    /// Returns the fully reassembled serialized response, ensuring it matches the expected total.
    pub fn finish(self) -> anyhow::Result<String> {
        if self.received.len() as u64 != self.total {
            anyhow::bail!("reassembled pagination response has {} bytes, expected {}", self.received.len(), self.total);
        }
        String::from_utf8(self.received).context("reassembled pagination response is not valid utf-8")
    }
}
