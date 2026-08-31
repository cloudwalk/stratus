//! Response pagination for oversized importer RPC responses.
//!
//! The importer endpoints (`stratus_getBlockAndReceipts` and `stratus_getBlockWithChanges`) can
//! produce responses larger than [`MAX_RESPONSE_SIZE_BYTES`] (leader) or
//! `EXTERNAL_RPC_MAX_RESPONSE_SIZE_BYTES` (follower), which makes the importer get stuck retrying
//! forever. This module implements a minimal, stateless pagination protocol to circumvent that:
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
//! the chunk budget so the whole envelope fits within the response size limits of both sides. Since
//! the envelope has exactly one top-level key, it always serializes first, allowing O(1) prefix
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
///
/// Both the leader's `max_response_body_size` and the follower's `max_response_size` count the
/// whole JSON-RPC response body, so a single margin covers both sides.
pub const MARGIN: u32 = 512;

/// Minimum accepted `chunk_budget`; smaller values could stall reassembly to a crawl.
pub const MIN_CHUNK_BUDGET: u64 = 64;

/// Minimum response size limit for pagination to make progress on either side of the connection:
/// the envelope margin, one minimum chunk, and headroom for the envelope and JSON-RPC wrappers.
///
/// Limits below this floor are rejected at startup ([`validate_response_size_limit`]): the leader
/// would clamp the effective chunk budget below [`MIN_CHUNK_BUDGET`] (stalling reassembly with
/// empty chunks), and the follower would request a budget the leader rejects as invalid.
pub const MIN_RESPONSE_SIZE_BYTES: u32 = MARGIN + MIN_CHUNK_BUDGET as u32 + 128;

/// Validates a response size limit (leader `max_response_size` or follower
/// `max_response_size_bytes`) is large enough for pagination to make progress.
pub fn validate_response_size_limit(limit: u32) -> anyhow::Result<()> {
    if limit < MIN_RESPONSE_SIZE_BYTES {
        anyhow::bail!("response size limit of {limit} bytes is too small for pagination: must be at least {MIN_RESPONSE_SIZE_BYTES} bytes");
    }
    Ok(())
}

/// Maximum bytes preallocated for reassembly, to avoid OOM on a bogus `total` from a malicious peer.
const MAX_REASSEMBLY_PREALLOC: usize = 1024 * 1024;

/// Hard cap on the total size of a reassembled paginated response, as defense against a
/// malicious or buggy leader advertising an arbitrarily large `total`.
pub const MAX_REASSEMBLY_TOTAL: u64 = 512 * 1024 * 1024;

// -----------------------------------------------------------------------------
// Request params
// -----------------------------------------------------------------------------

/// Optional second parameter sent by pagination-capable followers.
///
/// Presence of the object is the capability signal: old leaders ignore extra params, and the
/// leader never paginates without it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    /// Byte offset of the requested chunk within the serialized response.
    pub offset: u64,

    /// Maximum size (in bytes) of the JSON-escaped chunk the follower can receive in one response.
    pub chunk_budget: u64,
}

/// Extracts the optional [`PaginationParams`] from the remaining request params sequence.
pub fn parse_request(mut params: ParamsSequence<'_>) -> Result<Option<PaginationParams>, RpcError> {
    let parsed = params.optional_next::<PaginationParams>().map_err(|e| RpcError::ParameterDecodeError {
        rust_type: "PaginationParams",
        decode_error: e.data().map(|d| d.to_string()).unwrap_or_default(),
    })?;

    match parsed {
        Some(pagination) if pagination.chunk_budget < MIN_CHUNK_BUDGET => Err(RpcError::ParameterInvalid),
        other => Ok(other),
    }
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
/// The effective chunk budget is the minimum of what the follower requested and what the leader
/// can send, so the envelope always fits within the response size limits of both sides.
pub fn respond(value: JsonValue, pagination: Option<PaginationParams>, max_response_bytes: u32) -> Result<Box<RawValue>, StratusError> {
    let raw = to_raw_value(&value).map_err(|e| StratusError::Unexpected(crate::eth::types::UnexpectedError::Unexpected(anyhow::anyhow!(e))))?;

    let Some(pagination) = pagination else {
        return Ok(raw);
    };

    let full = raw.get();
    let budget = effective_chunk_budget(pagination.chunk_budget, max_response_bytes);

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

/// Returns the chunk budget bounded by both the follower request and the leader response limit.
fn effective_chunk_budget(requested: u64, max_response_bytes: u32) -> usize {
    let leader_cap = max_response_bytes.saturating_sub(MARGIN) as u64;
    requested.min(leader_cap) as usize
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
// Tests
// -----------------------------------------------------------------------------

/// Builds pagination request params for the follower side.
pub fn request_params(offset: u64, chunk_budget: u64) -> JsonValue {
    to_json_value(PaginationParams { offset, chunk_budget })
}

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

    #[test]
    fn respond_without_pagination_is_byte_identical() {
        let value = json!({"block": "abc", "receipts": [1, 2, 3]});
        let raw = respond(value.clone(), None, 1024).expect("respond");
        assert_eq!(raw.get(), serde_json::to_string(&value).expect_infallible());
    }

    #[test]
    fn respond_with_fitting_response_returns_full() {
        let value = json!({"block": "abc"});
        let raw = respond(value.clone(), Some(PaginationParams { offset: 0, chunk_budget: 1024 }), 1024).expect("respond");
        assert_eq!(raw.get(), serde_json::to_string(&value).expect_infallible());
    }

    #[test]
    fn respond_with_oversized_response_returns_envelope() {
        let value = json!({"block": "a somewhat long value that will not fit"});
        let raw = respond(value.clone(), Some(PaginationParams { offset: 0, chunk_budget: 16 }), 10_000).expect("respond");

        let full = serde_json::to_string(&value).expect_infallible();
        assert!(is_envelope(raw.get()));
        let envelope = parse_envelope(raw.get()).expect("parse envelope");
        assert_eq!(envelope.total, full.len() as u64);
        assert_eq!(envelope.chunk, &full[..envelope.chunk.len()]);
    }

    #[test]
    fn respond_envelope_chunks_cover_whole_response() {
        let value = json!({"block": "value with \"quotes\" to force escaping", "receipts": [1, 2, 3]});
        let full = serde_json::to_string(&value).expect_infallible();
        let budget = 8u64;

        let mut reassembler = Reassembler::new(0);
        let mut offset = 0;
        while offset < full.len() as u64 {
            let raw = respond(value.clone(), Some(PaginationParams { offset, chunk_budget: budget }), u32::MAX).expect("respond");
            assert!(is_envelope(raw.get()), "expected envelope at offset {offset}");
            let envelope = parse_envelope(raw.get()).expect("parse envelope");
            if offset == 0 {
                reassembler = Reassembler::new(envelope.total);
            }
            reassembler.push(envelope).expect("push");
            offset = reassembler.next_offset();
        }

        assert!(reassembler.next_offset() == full.len() as u64);
        assert_eq!(reassembler.finish().expect("finish"), full);
    }

    #[test]
    fn respond_with_offset_beyond_response_fails() {
        let value = json!({"block": "abc"});
        let error = respond(value, Some(PaginationParams { offset: 100, chunk_budget: 2 }), 10_000).expect_err("should fail");
        assert!(matches!(error, StratusError::RPC(RpcError::ParameterInvalid)));
    }

    #[test]
    fn respond_with_misaligned_offset_fails_instead_of_panicking() {
        let value = json!({"block": "\u{65e5}\u{672c}\u{8a9e} unicode content that will not fit", "receipts": [1, 2, 3]});
        let full = serde_json::to_string(&value).expect_infallible();

        // first byte strictly inside a multi-byte char: a valid index that is not a boundary
        let misaligned = full
            .char_indices()
            .find_map(|(i, ch)| (ch.len_utf8() > 1).then_some(i + 1))
            .expect("multi-byte char");
        assert!(!full.is_char_boundary(misaligned));

        let error = respond(
            value,
            Some(PaginationParams {
                offset: misaligned as u64,
                chunk_budget: 8,
            }),
            10_000,
        )
        .expect_err("should fail");
        assert!(matches!(error, StratusError::RPC(RpcError::ParameterInvalid)));
    }

    #[test]
    fn effective_chunk_budget_is_bounded_by_both_sides() {
        assert_eq!(effective_chunk_budget(1000, 10_000), 1000);
        assert_eq!(effective_chunk_budget(10_000, 1000), 488); // 1000 - MARGIN
        assert_eq!(effective_chunk_budget(10_000, 100), 0);
    }

    #[test]
    fn parse_request_parses_valid_params() {
        let params = jsonrpsee::types::Params::new(Some(r#"["0x1", {"offset": 5, "chunk_budget": 1024}]"#));
        let mut sequence = params.sequence();
        sequence.optional_next::<String>().expect("parse first").expect("present");
        let pagination = parse_request(sequence).expect("parse request");
        let pagination = pagination.expect("present");
        assert_eq!(pagination.offset, 5);
        assert_eq!(pagination.chunk_budget, 1024);
    }

    #[test]
    fn parse_request_without_params_returns_none() {
        let params = jsonrpsee::types::Params::new(Some(r#"["0x1"]"#));
        let mut sequence = params.sequence();
        sequence.optional_next::<String>().expect("parse first").expect("present");
        assert!(parse_request(sequence).expect("parse request").is_none());
    }

    #[test]
    fn parse_request_rejects_invalid_params() {
        let params = jsonrpsee::types::Params::new(Some(r#"["0x1", {"offset": 5}]"#));
        let mut sequence = params.sequence();
        sequence.optional_next::<String>().expect("parse first").expect("present");
        assert!(matches!(parse_request(sequence), Err(RpcError::ParameterDecodeError { .. })));

        let params = jsonrpsee::types::Params::new(Some(r#"["0x1", {"offset": 5, "chunk_budget": 1}]"#));
        let mut sequence = params.sequence();
        sequence.optional_next::<String>().expect("parse first").expect("present");
        assert!(matches!(parse_request(sequence), Err(RpcError::ParameterInvalid)));
    }

    #[test]
    fn envelope_wire_format_is_stable() {
        let envelope = PaginatedResponse {
            __stratus_paginated__: PaginationEnvelope {
                total: 42,
                chunk: "a\"b".to_owned(),
            },
        };
        let serialized = serde_json::to_string(&envelope).expect_infallible();

        // Single top-level key always serializes first; pins the prefix sniffing contract.
        assert!(serialized.starts_with(ENVELOPE_PREFIX), "serialized: {serialized}");
        assert!(serialized.contains(r#""__stratus_paginated__""#));

        // Round-trip.
        let parsed: PaginatedResponse = serde_json::from_str(&serialized).expect_infallible();
        assert_eq!(parsed.__stratus_paginated__.total, 42);
        assert_eq!(parsed.__stratus_paginated__.chunk, "a\"b");
    }

    #[test]
    fn is_envelope_rejects_normal_responses() {
        assert!(!is_envelope("{\"block\": 1, \"receipts\": []}"));
        assert!(!is_envelope("[{\"header\": 1}, {\"changes\": 2}]"));
        assert!(!is_envelope("null"));
        assert!(is_envelope("{\"__stratus_paginated__\":{\"chunk\":\"a\",\"total\":1}}"));
    }

    #[test]
    fn reassembler_validates_progress_and_totals() {
        let mut reassembler = Reassembler::new(10);
        assert!(
            !reassembler
                .push(PaginationEnvelope {
                    total: 10,
                    chunk: "abcd".to_owned()
                })
                .expect("push")
        );
        assert_eq!(reassembler.next_offset(), 4);
        assert!(
            reassembler
                .push(PaginationEnvelope {
                    total: 10,
                    chunk: String::new()
                })
                .is_err()
        );
        assert!(
            reassembler
                .push(PaginationEnvelope {
                    total: 11,
                    chunk: "efg".to_owned()
                })
                .is_err()
        );
        assert!(
            reassembler
                .push(PaginationEnvelope {
                    total: 10,
                    chunk: "efghij".to_owned()
                })
                .expect("push")
        );
        assert_eq!(reassembler.finish().expect("finish"), "abcdefghij");
    }

    #[test]
    fn reassembler_finish_rejects_size_mismatch() {
        let mut reassembler = Reassembler::new(10);
        reassembler
            .push(PaginationEnvelope {
                total: 10,
                chunk: "abc".to_owned(),
            })
            .expect("push");
        assert!(reassembler.finish().is_err());
    }
}

// -----------------------------------------------------------------------------
// Wire tests: real jsonrpsee server + client through the HTTP transport
// -----------------------------------------------------------------------------

#[cfg(test)]
mod wire_tests {
    use std::sync::Arc;
    use std::sync::RwLock;
    use std::time::Duration;

    use jsonrpsee::server::RpcModule;
    use jsonrpsee::server::Server;

    use super::parse_request;
    use super::respond;
    use crate::alias::JsonValue;
    use crate::eth::rpc::BlockchainClient;
    use crate::eth::rpc::next_rpc_param;
    use crate::eth::rpc::types::BlockFilter;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::ExternalBlockWithReceipts;
    use crate::eth::types::ExternalReceipt;
    use crate::eth::types::StratusError;
    use crate::ext::to_json_value;
    use crate::utils::test_utils::fake_first;
    use crate::utils::test_utils::fake_list;

    /// Response limit for both leader and follower sides in the tests below.
    const MAX_RESPONSE_BYTES: u32 = 2048;

    /// Builds an importer response well above the response limits.
    fn big_block_with_receipts() -> ExternalBlockWithReceipts {
        let mut block = fake_first::<ExternalBlockWithReceipts>();
        block.receipts = fake_list::<ExternalReceipt>(200);
        block
    }

    /// Asserts the serialized form of the test value cannot fit in the response limits.
    #[test]
    fn test_value_is_oversized() {
        let value = to_json_value(big_block_with_receipts());
        let serialized = serde_json::to_string(&value).expect("serialize");
        assert!(
            serialized.len() > MAX_RESPONSE_BYTES as usize,
            "test value is not oversized: {} bytes",
            serialized.len()
        );
    }

    #[tokio::test]
    async fn oversized_importer_response_is_paginated_over_the_wire() {
        let expected = big_block_with_receipts();
        let storage = Arc::new(RwLock::new(to_json_value(expected.clone())));

        // leader with a tiny response limit, using the same handler shape as the real one
        let server_config = jsonrpsee::server::ServerConfig::builder().max_response_body_size(MAX_RESPONSE_BYTES).build();
        let server = Server::builder().set_config(server_config).build("127.0.0.1:0").await.expect("build server");
        let addr = server.local_addr().expect("server addr");

        let mut module = RpcModule::new(Arc::clone(&storage));
        module
            .register_method("net_listening", |_, _, _| Ok::<_, StratusError>(true))
            .expect("register net_listening");
        module
            .register_method("stratus_getBlockAndReceipts", |params, storage, _| {
                let (sequence, _filter) = next_rpc_param::<BlockFilter>(params.sequence())?;
                let pagination = parse_request(sequence)?;
                let value = storage.read().expect("read storage").clone();
                respond(value, pagination, MAX_RESPONSE_BYTES)
            })
            .expect("register stratus_getBlockAndReceipts");
        let _server_handle = server.start(module);

        // follower with a tiny response limit, like the importer uses
        let url = format!("http://{addr}");
        let client = BlockchainClient::new_http(&url, Duration::from_secs(10), MAX_RESPONSE_BYTES)
            .await
            .expect("build client");

        let fetched = client.fetch_block_and_receipts(BlockNumber::from(1)).await.expect("fetch block");
        assert_eq!(fetched.expect("block present"), expected);
    }

    #[tokio::test]
    async fn old_leader_without_pagination_still_fails_as_before() {
        let storage = Arc::new(RwLock::new(to_json_value(big_block_with_receipts())));

        // old leader: ignores the extra pagination parameter, returns the full response
        let server_config = jsonrpsee::server::ServerConfig::builder().max_response_body_size(MAX_RESPONSE_BYTES).build();
        let server = Server::builder().set_config(server_config).build("127.0.0.1:0").await.expect("build server");
        let addr = server.local_addr().expect("server addr");

        let mut module = RpcModule::new(Arc::clone(&storage));
        module
            .register_method("net_listening", |_, _, _| Ok::<_, StratusError>(true))
            .expect("register net_listening");
        module
            .register_method("stratus_getBlockAndReceipts", |_, storage, _| {
                let value = storage.read().expect("read storage").clone();
                Ok(value) as Result<JsonValue, StratusError>
            })
            .expect("register stratus_getBlockAndReceipts");
        let _server_handle = server.start(module);

        // the oversized response is rejected by the server, same as before pagination existed
        let url = format!("http://{addr}");
        let client = BlockchainClient::new_http(&url, Duration::from_secs(10), MAX_RESPONSE_BYTES)
            .await
            .expect("build client");

        let error = client.fetch_block_and_receipts(BlockNumber::from(1)).await.expect_err("fetch should fail");
        assert!(error.to_string().contains("failed to fetch block with receipts"));
    }

    #[tokio::test]
    async fn null_response_means_block_not_available() {
        let storage = Arc::new(RwLock::new(JsonValue::Null));

        // leader answering null: the block is not available yet
        let server_config = jsonrpsee::server::ServerConfig::builder().max_response_body_size(MAX_RESPONSE_BYTES).build();
        let server = Server::builder().set_config(server_config).build("127.0.0.1:0").await.expect("build server");
        let addr = server.local_addr().expect("server addr");

        let mut module = RpcModule::new(Arc::clone(&storage));
        module
            .register_method("net_listening", |_, _, _| Ok::<_, StratusError>(true))
            .expect("register net_listening");
        module
            .register_method("stratus_getBlockAndReceipts", |_, storage, _| {
                let value = storage.read().expect("read storage").clone();
                Ok(value) as Result<JsonValue, StratusError>
            })
            .expect("register stratus_getBlockAndReceipts");
        let _server_handle = server.start(module);

        let url = format!("http://{addr}");
        let client = BlockchainClient::new_http(&url, Duration::from_secs(10), MAX_RESPONSE_BYTES)
            .await
            .expect("build client");

        let fetched = client.fetch_block_and_receipts(BlockNumber::from(1)).await.expect("fetch block");
        assert!(fetched.is_none(), "null response must deserialize to Ok(None)");
    }
}
