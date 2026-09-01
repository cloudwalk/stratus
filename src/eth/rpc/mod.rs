//! Ethereum JSON-RPC server.

mod config;
mod context;
pub mod middleware;
pub(crate) mod pagination;
mod parser;
mod server;
mod subscriptions;
pub mod types;

pub use config::RpcServerConfig;
pub use context::RpcContext;
pub use middleware::RpcHttpMiddleware;
pub use middleware::RpcMiddleware;
use parser::next_rpc_param;
use parser::next_rpc_param_or_default;
use parser::parse_rpc_rlp;
pub use server::Server;
pub use subscriptions::RpcSubscriptions;
pub use types::BlockFilter;
pub use types::BlockTimestampFilter;
pub use types::BlockTimestampSeekMode;
pub use types::LogFilter;
pub use types::LogFilterInput;
pub use types::LogFilterInputTopic;
pub use types::MulticallError;
pub use types::RpcClientApp;
pub use types::RpcError;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

// Tests of the public pagination API; tests of its private helpers live in `pagination.rs`.
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::pagination::MARGIN;
    use super::pagination::MIN_RESPONSE_SIZE_BYTES;
    use super::pagination::PaginatedResponse;
    use super::pagination::PaginationEnvelope;
    use super::pagination::PaginationParams;
    use super::pagination::Reassembler;
    use super::pagination::is_envelope;
    use super::pagination::parse_envelope;
    use super::pagination::parse_request;
    use super::pagination::respond;
    use super::pagination::validate_response_size_limit;
    use super::types::RpcError;
    use crate::eth::types::StratusError;
    use crate::ext::InfallibleExt;

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
    fn respond_with_response_exactly_at_budget_returns_full() {
        // a response whose serialized length is exactly the effective budget is not paginated
        let value = json!({"block": "some content"});
        let full = serde_json::to_string(&value).expect("serialize");
        let requested = full.len() as u64;

        // leader limit chosen so both sides of the effective budget resolve to the response size
        let result = respond(
            value,
            Some(PaginationParams {
                offset: 0,
                chunk_budget: requested,
            }),
            full.len() as u32 + MARGIN,
        )
        .expect("should respond");
        assert_eq!(result.get(), full);
        assert!(!is_envelope(result.get()));
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
    fn validate_response_size_limit_accepts_floor_and_rejects_below() {
        validate_response_size_limit(MIN_RESPONSE_SIZE_BYTES).expect("floor should be accepted");
        validate_response_size_limit(MIN_RESPONSE_SIZE_BYTES + 1).expect("above floor should be accepted");
        let error = validate_response_size_limit(MIN_RESPONSE_SIZE_BYTES - 1).expect_err("below floor should be rejected");
        assert!(error.to_string().contains("too small for pagination"));
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
        assert!(serialized.starts_with(r#"{"__stratus_paginated__":"#), "serialized: {serialized}");
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

// Wire tests of the public pagination API: real jsonrpsee server + client through HTTP.
#[cfg(test)]
mod wire_tests {
    use std::sync::Arc;
    use std::sync::RwLock;
    use std::time::Duration;

    use jsonrpsee::server::RpcModule;
    use jsonrpsee::server::Server;
    use serde_json::json;

    use super::pagination::MAX_REASSEMBLY_TOTAL;
    use super::pagination::parse_request;
    use super::pagination::respond;
    use super::parser::next_rpc_param;
    use super::types::BlockFilter;
    use crate::alias::JsonValue;
    use crate::eth::follower::importer::BlockchainClient;
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

    #[tokio::test]
    async fn envelope_total_above_reassembly_cap_is_rejected() {
        // malicious or buggy leader advertising a total beyond the reassembly cap
        let storage = Arc::new(RwLock::new(json!({
            "__stratus_paginated__": { "total": MAX_REASSEMBLY_TOTAL + 1, "chunk": "a" }
        })));

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

        let error = client.fetch_block_and_receipts(BlockNumber::from(1)).await.expect_err("fetch should fail");
        assert!(format!("{error:?}").contains("exceeds the reassembly cap"));
    }

    #[tokio::test]
    async fn paginated_stream_switching_to_normal_response_fails() {
        // leader that answers the first chunk and then a normal response mid-stream
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        let calls = Arc::new(AtomicUsize::new(0));

        let server_config = jsonrpsee::server::ServerConfig::builder().max_response_body_size(MAX_RESPONSE_BYTES).build();
        let server = Server::builder().set_config(server_config).build("127.0.0.1:0").await.expect("build server");
        let addr = server.local_addr().expect("server addr");

        let mut module = RpcModule::new(Arc::clone(&calls));
        module
            .register_method("net_listening", |_, _, _| Ok::<_, StratusError>(true))
            .expect("register net_listening");
        module
            .register_method("stratus_getBlockAndReceipts", |_, calls, _| {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(json!({ "__stratus_paginated__": { "total": 100, "chunk": "a".repeat(50) } })) as Result<JsonValue, StratusError>
                } else {
                    Ok(json!({ "block": "abc" })) as Result<JsonValue, StratusError>
                }
            })
            .expect("register stratus_getBlockAndReceipts");
        let _server_handle = server.start(module);

        let url = format!("http://{addr}");
        let client = BlockchainClient::new_http(&url, Duration::from_secs(10), MAX_RESPONSE_BYTES)
            .await
            .expect("build client");

        let error = client.fetch_block_and_receipts(BlockNumber::from(1)).await.expect_err("fetch should fail");
        assert!(format!("{error:?}").contains("expected paginated chunk but got normal response"));
    }
}
