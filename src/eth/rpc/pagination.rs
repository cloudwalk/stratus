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
//!   answers with [`PaginatedResponse`], an envelope carrying one raw fragment (`chunk`) of the
//!   serialized response. The follower keeps requesting further offsets and reassembles the
//!   fragments until it has the full serialized response, which it then deserializes normally.
//!
//! Wire format (leader result when paginating):
//!
//! ```json
//! {"__stratus_paginated__": {"total": 12345, "chunk": "{\"block\":..."}}
//! ```
//!
//! The `chunk` string is a raw slice of the serialized response; its JSON-escaped form is bounded by
//! the chunk budget so the whole envelope fits within the leader's response size limit. Since the
//! envelope has exactly one top-level key, it always serializes first, allowing O(1) prefix
//! detection ([`is_envelope`]) on the follower side without parsing the (potentially huge) response.

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
const ENVELOPE_PREFIX: &str = "{\"__stratus_paginated__\":";

/// Bytes reserved for the JSON-RPC response envelope (`{"jsonrpc":"2.0","id":..,"result":..}`)
/// and the pagination envelope object itself, on top of the chunk payload.
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

// -----------------------------------------------------------------------------
// Request params
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Response envelope
// -----------------------------------------------------------------------------

/// One chunk of the serialized response, plus its total size in bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationEnvelope {
    /// Total size in bytes of the fully serialized response.
    pub total: u64,

    /// Raw fragment of the serialized response, starting at the requested offset.
    pub chunk: String,
}

/// Leader result when the response does not fit in a single message.
///
/// The sentinel field name doubles as the JSON key; having a single top-level field guarantees it
/// serializes first, which [`is_envelope`] relies on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse {
    pub __stratus_paginated__: PaginationEnvelope,
}

/// Returns whether a raw result is a paginated envelope (O(1), no parsing).
pub fn is_envelope(raw: &str) -> bool {
    raw.starts_with(ENVELOPE_PREFIX)
}

/// Parses a raw result assumed to be a paginated envelope.
pub fn parse_envelope(raw: &str) -> anyhow::Result<PaginationEnvelope> {
    let response = serde_json::from_str::<PaginatedResponse>(raw).map_err(|e| anyhow::anyhow!(e).context("failed to parse paginated response envelope"))?;
    Ok(response.__stratus_paginated__)
}

// -----------------------------------------------------------------------------
// Leader side
// -----------------------------------------------------------------------------

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
    if offset > full.len() || !full.is_char_boundary(offset) {
        tracing::error!(
            offset,
            total = full.len(),
            "pagination offset is beyond the response size or not a char boundary"
        );
        return Err(RpcError::ParameterInvalid.into());
    }

    let chunk = take_escaped_chunk(full, offset, budget);
    tracing::debug!(
        total = full.len(),
        offset,
        chunk_bytes = chunk.len(),
        budget,
        "paginating oversized rpc response"
    );

    let envelope = PaginatedResponse {
        __stratus_paginated__: PaginationEnvelope {
            total: full.len() as u64,
            chunk: chunk.to_owned(),
        },
    };
    to_raw_value(&envelope).map_err(|e| StratusError::Unexpected(crate::eth::types::UnexpectedError::Unexpected(anyhow::anyhow!(e))))
}

/// Returns the raw fragment of `full` starting at `offset` whose JSON-escaped form fits in `budget`.
///
/// `offset` must be a char boundary of `full` (the caller is responsible for validating
/// untrusted offsets before slicing); the returned cut is also always at a char boundary, and
/// each fragment is independently escapable, so concatenating the unescaped fragments reassembles
/// `full` exactly. Guaranteed to be non-empty when `offset < full.len()` and `budget >= 6` (the
/// largest escape expansion of a single char).
fn take_escaped_chunk(full: &str, offset: usize, budget: usize) -> &str {
    debug_assert!(full.is_char_boundary(offset));

    let mut end = offset;
    let mut escaped_len = 0;
    for ch in full[offset..].chars() {
        let char_escaped_len = escaped_len_of_char(ch);
        if escaped_len + char_escaped_len > budget {
            break;
        }
        escaped_len += char_escaped_len;
        end += ch.len_utf8();
    }

    &full[offset..end]
}

/// Returns how many bytes a char occupies when JSON-escaped inside a string.
///
/// Mirrors `serde_json`'s escape table: `"` and `\` become two-char escapes, the common control
/// chars (`\b`, `\t`, `\n`, `\f`, `\r`) become two-char escapes, other control chars below 0x20
/// become six-char `\u00XX` escapes, and everything else (including 0x7F and multi-byte UTF-8) is
/// passed through. Pinned by tests against `serde_json::to_string`.
fn escaped_len_of_char(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\u{08}' | '\u{09}' | '\u{0a}' | '\u{0c}' | '\u{0d}' => 2,
        ch if (ch as u32) < 0x20 => 6,
        ch => ch.len_utf8(),
    }
}

// -----------------------------------------------------------------------------
// Follower side
// -----------------------------------------------------------------------------

/// Builds pagination request params for the follower side.
pub fn request_params(offset: u64) -> JsonValue {
    to_json_value(PaginationParams { offset })
}

/// Progressive reassembly of a paginated response, with validation against a malicious peer.
#[derive(Debug)]
pub struct Reassembler {
    total: u64,
    received: String,
}

impl Reassembler {
    /// Creates a reassembler for a response of `total` bytes.
    pub fn new(total: u64) -> Self {
        let capacity = usize::try_from(total).unwrap_or(usize::MAX).min(MAX_REASSEMBLY_PREALLOC);
        Self {
            total,
            received: String::with_capacity(capacity),
        }
    }

    /// Appends a chunk and returns whether the response is complete.
    pub fn push(&mut self, envelope: PaginationEnvelope) -> anyhow::Result<bool> {
        if envelope.total != self.total {
            anyhow::bail!("pagination total changed mid-response: expected {}, got {}", self.total, envelope.total);
        }

        let done = self.received.len() as u64 + envelope.chunk.len() as u64 >= self.total;
        if !done && envelope.chunk.is_empty() {
            anyhow::bail!("received empty pagination chunk with {} of {} bytes", self.received.len(), self.total);
        }

        self.received.push_str(&envelope.chunk);
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
        Ok(self.received)
    }
}

// -----------------------------------------------------------------------------
// Tests (private helpers only; tests of the public API live in `mod.rs`)
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ext::InfallibleExt;

    /// Strings covering every escape class of `serde_json`'s table.
    const ADVERSARIAL_STRINGS: &[&str] = &[
        "",
        "plain",
        "with \"quotes\" inside",
        "with \\backslash\\",
        "control \u{08}\u{09}\u{0a}\u{0c}\u{0d}",
        "control \u{00}\u{01}\u{1f}",
        "unicode héllo wörld 日本語",
        "mixed \"\\\"\u{07}\u{7f} emoji \u{1f600}",
    ];

    #[test]
    fn escaped_len_of_char_matches_serde_json() {
        for input in ADVERSARIAL_STRINGS {
            let serialized = serde_json::to_string(input).expect_infallible();
            let expected = serialized.len() - 2; // exclude surrounding quotes
            let actual: usize = input.chars().map(escaped_len_of_char).sum();
            assert_eq!(actual, expected, "mismatch for {input:?}");
        }
    }

    #[test]
    fn take_escaped_chunk_reassembles_exactly() {
        let full = serde_json::to_string(&json!({
            "block": "with \"quotes\" and \\backslashes\\ and \u{07} control and unicode 日本",
            "receipts": [1, 2, 3],
        }))
        .expect_infallible();

        for budget in [6, 7, 13, 64, 1024] {
            let mut reassembled = String::new();
            let mut offset = 0;
            while offset < full.len() {
                let chunk = take_escaped_chunk(&full, offset, budget);
                assert!(!chunk.is_empty(), "no progress at offset {offset} with budget {budget}");
                assert!(
                    serde_json::to_string(chunk).expect_infallible().len() - 2 <= budget,
                    "chunk exceeds budget {budget}"
                );
                reassembled.push_str(chunk);
                offset += chunk.len();
            }
            assert_eq!(reassembled, full, "reassembled mismatch with budget {budget}");
        }
    }

    #[test]
    fn take_escaped_chunk_respects_offset() {
        let full = "0123456789";
        assert_eq!(take_escaped_chunk(full, 0, 4), "0123");
        assert_eq!(take_escaped_chunk(full, 4, 4), "4567");
        assert_eq!(take_escaped_chunk(full, 8, 4), "89");
        assert_eq!(take_escaped_chunk(full, 10, 4), "");
    }
}
