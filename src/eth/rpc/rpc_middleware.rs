//! Track RPC requests and responses using metrics and traces.

use std::future::Future;
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use futures::future::BoxFuture;
use jsonrpsee::core::middleware::Batch;
use jsonrpsee::core::middleware::BatchEntry;
use jsonrpsee::server::middleware::rpc::RpcService;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
#[cfg(feature = "metrics")]
use jsonrpsee::server::ConnectionGuard;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::Id;
use jsonrpsee::types::Params;
use jsonrpsee::BatchResponseBuilder;
use jsonrpsee::MethodResponse;
use pin_project::pin_project;
use tracing::field;
use tracing::info_span;
use tracing::Level;
use tracing::Span;

use crate::alias::JsonValue;
use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::codegen::SoliditySignature;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
#[cfg(feature = "metrics")]
use crate::eth::primitives::ErrorCode;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcClientApp;
use crate::event_with;
use crate::ext::from_json_str;
use crate::ext::to_json_string;
#[cfg(feature = "metrics")]
use crate::if_else;
use crate::infra::metrics;
use crate::infra::tracing::new_cid;
use crate::infra::tracing::SpanExt;
use crate::infra::tracing::TracingExt;
use crate::GlobalState;

// -----------------------------------------------------------------------------
// Request handling
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RpcMiddleware {
    service: Arc<RpcService>,
}

impl RpcMiddleware {
    pub fn new(service: RpcService) -> Self {
        Self { service: Arc::new(service) }
    }
}

impl RpcServiceT for RpcMiddleware {
    type BatchResponse = MethodResponse;
    type MethodResponse = MethodResponse;
    type NotificationResponse = MethodResponse;

    fn batch<'a>(&self, batch: Batch<'a>) -> impl Future<Output = Self::BatchResponse> + Send + 'a {
		let mut batch_rp = BatchResponseBuilder::new_with_limit(1024 * 1024 * 100); // 100 MB
		let service = self.clone();
		async move {
			let mut got_notification = false;

			for batch_entry in batch.into_iter() {
				match batch_entry {
					Ok(BatchEntry::Call(req)) => {
						let rp = service.call(req).await;
						if let Err(err) = batch_rp.append(rp) {
							return err;
						}
					}
					Ok(BatchEntry::Notification(n)) => {
						got_notification = true;
						service.notification(n).await;
					}
					Err(err) => {
						let (err, id) = err.into_parts();
						let rp = MethodResponse::error(id, err);
						if let Err(err) = batch_rp.append(rp) {
							return err;
						}
					}
				}
			}

			// If the batch is empty and we got a notification, we return an empty response.
			if batch_rp.is_empty() && got_notification {
				MethodResponse::notification()
			}
			// An empty batch is regarded as an invalid request here.
			else {
				MethodResponse::from_batch(batch_rp.finish())
			}
		}
	}

    // TODO
    fn notification<'a>(&self, n: jsonrpsee::core::middleware::Notification<'a>) -> impl Future<Output = Self::NotificationResponse> + Send + 'a {
        self.service.notification(n)
    }

    fn call<'a>(&self, mut request: jsonrpsee::types::Request<'a>) -> impl Future<Output = Self::MethodResponse> + Send + 'a {
        let span = info_span!(
            parent: None,
            "rpc::request",
            cid = %new_cid(),
            rpc_client = field::Empty,
            rpc_id = field::Empty,
            rpc_method = field::Empty,
            rpc_tx_hash = field::Empty,
            rpc_tx_from = field::Empty,
            rpc_tx_to = field::Empty,
            rpc_tx_nonce = field::Empty,
            rpc_tx_contract = field::Empty,
            rpc_tx_function = field::Empty
        );
        let middleware_enter = span.enter();

        // extract request data
        let method = request.method_name().to_owned();
        let mut tx = None;

        let params_clone = request.params().clone();

        if method == "eth_sendRawTransaction" {
            let tx_data_result = next_rpc_param::<Bytes>(params_clone.sequence());
            if let Ok((_, tx_data)) = tx_data_result {
                let decoded_tx_result = parse_rpc_rlp::<TransactionInput>(&tx_data);

                if let Ok(decoded_tx) = decoded_tx_result {
                    tx = TransactionTracingIdentifiers::from_raw_transaction(&decoded_tx).ok();

                    request.extensions_mut().insert(tx_data);
                    request.extensions_mut().insert(decoded_tx);
                }
            }
        } else {
            tx = match method.as_str() {
                "eth_call" | "eth_estimateGas" => TransactionTracingIdentifiers::from_call(params_clone.clone()).ok(),
                "eth_getTransactionByHash" | "eth_getTransactionReceipt" => TransactionTracingIdentifiers::from_transaction_query(params_clone.clone()).ok(),
                _ => None,
            };
        }

        let is_admin = request.extensions.is_admin();

        let client = if let Some(tx_client) = tx.as_ref().and_then(|tx| tx.client.as_ref()) {
            request.extensions_mut().insert(tx_client.clone());
            tx_client
        } else {
            request.extensions.rpc_client()
        }
        .to_owned();

        // trace event
        Span::with(|s| {
            s.rec_str("rpc_id", &request.id);
            s.rec_str("rpc_client", &client);
            s.rec_str("rpc_method", &method);
            if let Some(ref tx) = tx {
                tx.record_span(s);
            }
        });
        tracing::info!(
            rpc_client = %client,
            rpc_id = %request.id,
            rpc_method = %method,
            rpc_params = %to_json_string(&request.params),
            rpc_tx_hash = %tx.as_ref().and_then(|tx|tx.hash).or_empty(),
            rpc_tx_contract = %tx.as_ref().map(|tx|tx.contract).or_empty(),
            rpc_tx_function = %tx.as_ref().map(|tx|tx.function).or_empty(),
            rpc_tx_from = %tx.as_ref().and_then(|tx|tx.from).or_empty(),
            rpc_tx_to = %tx.as_ref().and_then(|tx|tx.to).or_empty(),
            is_admin = %is_admin,
            "rpc request"
        );

        // track metrics
        #[cfg(feature = "metrics")]
        {
            // started requests
            let tx_ref = tx.as_ref();
            metrics::inc_rpc_requests_started(&client, &method, tx_ref.map(|tx| tx.contract), tx_ref.map(|tx| tx.function));

            // active requests
            if let Some(guard) = request.extensions.get::<ConnectionGuard>() {
                let active = guard.max_connections() - guard.available_connections();
                metrics::set_rpc_requests_active(active as u64);
            }
        }

        // make span available to rpc-server
        drop(middleware_enter);
        request.extensions_mut().insert(span);

        let id = request.id.to_string();

        let future_response = reject_unknown_client(&client, request.id.clone()).unwrap_or(Box::pin(self.service.call(request)));
        RpcResponse {
            client,
            id,
            method: method.to_string(),
            tx,
            start: Instant::now(),
            future_response,
        }
    }
}

/// Returns an error JSON-RPC response if the client is not allowed to perform the current operation.
fn reject_unknown_client<'a>(client: &RpcClientApp, id: Id<'_>) -> Option<BoxFuture<'a, MethodResponse>> {
    if client.is_unknown() && !GlobalState::is_unknown_client_enabled() {
        return Some(Box::pin(StratusError::RPC(RpcError::ClientMissing).to_response_future(id)));
    }
    None
}

// -----------------------------------------------------------------------------
// Response handling
// -----------------------------------------------------------------------------

/// https://blog.adamchalmers.com/pin-unpin/
#[pin_project]
pub struct RpcResponse<'a> {
    // identifiers
    client: RpcClientApp,
    id: String,
    method: String,
    tx: Option<TransactionTracingIdentifiers>,

    // data
    start: Instant,
    #[pin]
    future_response: BoxFuture<'a, MethodResponse>,
}

impl Future for RpcResponse<'_> {
    type Output = MethodResponse;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        // poll future
        let resp = self.project();
        let mut response = resp.future_response.poll(cx);

        // when ready, track response before returning
        if let Poll::Ready(response) = &mut response {
            let elapsed = resp.start.elapsed();
            let middleware_enter = response.extensions().enter_middleware_span();

            // extract response data
            let response_success = response.is_success();
            let response_result: JsonValue = from_json_str(response.as_json().get());

            #[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
            let (level, error_code) = match response_result
                .get("error")
                .and_then(|v| v.get("code"))
                .and_then(|v| v.as_number())
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
            {
                Some(INTERNAL_ERROR_CODE) => (Level::ERROR, INTERNAL_ERROR_CODE),
                Some(code) => (Level::WARN, code),
                None => (Level::INFO, 0),
            };

            let log_tracing_event = || {
                event_with!(
                    level,
                    rpc_client = %resp.client,
                    rpc_id = %resp.id,
                    rpc_method = %resp.method,
                    rpc_tx_hash = %resp.tx.as_ref().and_then(|tx|tx.hash).or_empty(),
                    rpc_tx_contract = %resp.tx.as_ref().map(|tx|tx.contract).or_empty(),
                    rpc_tx_function = %resp.tx.as_ref().map(|tx|tx.function).or_empty(),
                    rpc_tx_from = %resp.tx.as_ref().and_then(|tx|tx.from).or_empty(),
                    rpc_tx_to = %resp.tx.as_ref().and_then(|tx|tx.to).or_empty(),
                    rpc_result = %response_result,
                    rpc_success = %response_success,
                    duration_us = %elapsed.as_micros(),
                    "rpc response"
                );
            };

            sentry::with_scope(
                |scope| {
                    scope.set_user(Some(sentry::User {
                        username: Some(resp.client.to_string()),
                        ..Default::default()
                    }));
                },
                log_tracing_event,
            );

            // track metrics
            #[cfg(feature = "metrics")]
            {
                let rpc_result = match response_result.get("result") {
                    Some(result) => if_else!(result.is_null(), metrics::LABEL_MISSING, metrics::LABEL_PRESENT),
                    None => StratusError::str_repr_from_err_code(error_code).unwrap_or("Unknown"),
                };

                let tx_ref = resp.tx.as_ref();
                metrics::inc_rpc_requests_finished(
                    elapsed,
                    &*resp.client,
                    resp.method.clone(),
                    tx_ref.map(|tx| tx.contract),
                    tx_ref.map(|tx| tx.function),
                    rpc_result,
                    error_code,
                    response.is_success(),
                );
            }

            // drop span because maybe jsonrpsee is keeping it alive
            drop(middleware_enter);
            response.extensions_mut().remove::<Span>();
        }

        response
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

struct TransactionTracingIdentifiers {
    pub client: Option<RpcClientApp>,
    pub hash: Option<Hash>,
    pub contract: ContractName,
    pub function: SoliditySignature,
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub nonce: Option<Nonce>,
}

impl TransactionTracingIdentifiers {
    /// eth_sendRawTransaction
    fn from_raw_transaction(decoded_tx: &TransactionInput) -> anyhow::Result<Self> {
        Ok(Self {
            client: None,
            hash: Some(decoded_tx.hash),
            contract: codegen::contract_name(&decoded_tx.to),
            function: codegen::function_sig(&decoded_tx.input),
            from: Some(decoded_tx.signer),
            to: decoded_tx.to,
            nonce: Some(decoded_tx.nonce),
        })
    }

    /// eth_call / eth_estimateGas
    fn from_call(params: Params) -> anyhow::Result<Self> {
        let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;
        Ok(Self {
            client: None,
            hash: None,
            contract: codegen::contract_name(&call.to),
            function: codegen::function_sig(&call.data),
            from: call.from,
            to: call.to,
            nonce: None,
        })
    }

    /// eth_getTransactionByHash / eth_getTransactionReceipt
    fn from_transaction_query(params: Params) -> anyhow::Result<Self> {
        let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
        Ok(Self {
            client: None,
            hash: Some(hash),
            contract: metrics::LABEL_MISSING,
            function: metrics::LABEL_MISSING,
            from: None,
            to: None,
            nonce: None,
        })
    }

    pub fn record_span(&self, span: Span) {
        span.rec_str("rpc_tx_contract", &self.contract);
        span.rec_str("rpc_tx_function", &self.function);

        if let Some(tx_hash) = self.hash {
            span.rec_str("rpc_tx_hash", &tx_hash);
        }
        if let Some(tx_from) = self.from {
            span.rec_str("rpc_tx_from", &tx_from);
        }
        if let Some(tx_to) = self.to {
            span.rec_str("rpc_tx_to", &tx_to);
        }
        if let Some(tx_nonce) = self.nonce {
            span.rec_str("rpc_tx_nonce", &tx_nonce);
        }
    }
}
