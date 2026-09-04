//! Track RPC requests and responses using metrics and traces.

use std::future::Future;
use std::sync::Arc;
use std::task::Poll;
use std::time::Instant;

use futures::future::BoxFuture;
use jsonrpsee::BatchResponseBuilder;
use jsonrpsee::MethodResponse;
use jsonrpsee::core::middleware::Batch;
use jsonrpsee::core::middleware::BatchEntry;
#[cfg(feature = "metrics")]
use jsonrpsee::server::ConnectionGuard;
use jsonrpsee::server::middleware::rpc::RpcService;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::Id;
use jsonrpsee::types::Params;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use pin_project::pin_project;
use strum::Display;
use tracing::Level;
use tracing::Span;
use tracing::field;
use tracing::info_span;

use crate::GlobalState;
use crate::alias::JsonValue;
use crate::eth::codegen;
use crate::eth::codegen::ContractName;
use crate::eth::codegen::SoliditySignature;
use crate::eth::rpc::RpcClientApp;
use crate::eth::rpc::RpcContext;
use crate::eth::rpc::RpcError;
use crate::eth::rpc::middleware::multicall::MulticallInfo;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parser::RpcExtensionsExt;
use crate::eth::rpc::server::eth_send_raw_transaction;
use crate::eth::types::Address;
use crate::eth::types::CallInput;
#[cfg(feature = "metrics")]
use crate::eth::types::ErrorCode;
use crate::eth::types::Hash;
use crate::eth::types::Nonce;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;
use crate::event_with;
use crate::ext::from_json_str;
use crate::ext::to_json_string;
#[cfg(feature = "metrics")]
use crate::if_else;
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::infra::tracing::TracingExt;
use crate::infra::tracing::new_cid;

// -----------------------------------------------------------------------------
// Request handling
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RpcMiddleware {
    service: Arc<RpcService>,
    ctx: Arc<RpcContext>,
}

impl RpcMiddleware {
    pub fn new(service: RpcService, ctx: Arc<RpcContext>) -> Self {
        Self {
            service: Arc::new(service),
            ctx,
        }
    }
}

#[derive(Default, Clone, Copy, Display)]
enum RequestType {
    Batch,
    #[default]
    Single,
}

#[cfg(feature = "metrics")]
impl From<RequestType> for metrics::MetricLabelValue {
    fn from(value: RequestType) -> Self {
        value.to_string().into()
    }
}

impl RpcServiceT for RpcMiddleware {
    type BatchResponse = MethodResponse;
    type MethodResponse = MethodResponse;
    type NotificationResponse = MethodResponse;

    fn batch<'a>(&self, batch: Batch<'a>) -> impl Future<Output = Self::BatchResponse> + Send + 'a {
        let mut batch_rp = BatchResponseBuilder::new_with_limit(1024 * 1024 * 100); // 100 MB
        let service = self.clone();
        let batch_size = batch.len();

        tracing::info!(batch_size = batch_size, "processing RPC batch request");

        async move {
            let mut got_notification = false;
            let start = Instant::now();
            let mut call_count = 0;
            let mut notification_count = 0;
            let mut error_count = 0;

            for batch_entry in batch.into_iter() {
                match batch_entry {
                    Ok(BatchEntry::Call(mut req)) => {
                        call_count += 1;
                        tracing::debug!(method = req.method_name(), "processing batch call");
                        req.extensions_mut().insert(RequestType::Batch);
                        let rp = service.call(req).await;
                        if let Err(err) = batch_rp.append(rp) {
                            tracing::error!(error = ?err, "failed to append batch call response");
                            return err;
                        }
                    }
                    Ok(BatchEntry::Notification(n)) => {
                        notification_count += 1;
                        tracing::debug!(method = n.method_name(), "processing batch notification");
                        got_notification = true;
                        service.notification(n).await;
                    }
                    Err(err) => {
                        error_count += 1;
                        tracing::warn!(error = ?err, "processing batch entry error");
                        let (err, id) = err.into_parts();
                        let rp = MethodResponse::error(id, err);
                        if let Err(err) = batch_rp.append(rp) {
                            tracing::error!(error = ?err, "failed to append batch error response");
                            return err;
                        }
                    }
                }
            }

            let elapsed = start.elapsed();
            if batch_rp.is_empty() && got_notification {
                tracing::info!(
                    elapsed_ms = elapsed.as_millis(),
                    calls = call_count,
                    notifications = notification_count,
                    errors = error_count,
                    "completed empty batch with notifications"
                );
                MethodResponse::notification()
            } else {
                tracing::info!(
                    elapsed_ms = elapsed.as_millis(),
                    calls = call_count,
                    notifications = notification_count,
                    errors = error_count,
                    "completed batch request"
                );
                MethodResponse::from_batch(batch_rp.finish())
            }
        }
    }

    fn notification<'a>(&self, n: jsonrpsee::core::middleware::Notification<'a>) -> impl Future<Output = Self::NotificationResponse> + Send + 'a {
        self.service.notification(n)
    }

    fn call<'a>(&self, mut request: jsonrpsee::types::Request<'a>) -> impl Future<Output = Self::MethodResponse> + Send + 'a {
        let request_type = request.extensions().get::<RequestType>().copied().unwrap_or_default();
        let is_admin = request.extensions.is_admin();
        let client = request.extensions.rpc_client().to_owned();
        let request_id = request.id();
        let method = request.method_name().to_owned();
        let request_params_str = to_json_string(&request.params);
        #[cfg(feature = "metrics")]
        if let Some(guard) = request.extensions.get::<ConnectionGuard>() {
            let active = guard.max_connections() - guard.available_connections();
            metrics::set_rpc_requests_active(active as u64);
        }

        if let Some(future_response) = reject_client(&client, request_id.clone()) {
            return RpcResponse {
                client,
                id: request_id.to_string(),
                method: method.to_string(),
                tx: None,
                start: Instant::now(),
                future_response,
            };
        }

        let span = info_span!(
            parent: None,
            "rpc::request",
            cid = %new_cid(),
            rpc_client = %client,
            rpc_id = %request_id,
            rpc_method = %method,
            rpc_tx_hash = field::Empty,
            rpc_tx_from = field::Empty,
            rpc_tx_to = field::Empty,
            rpc_tx_nonce = field::Empty,
            rpc_tx_contract = field::Empty,
            rpc_tx_function = field::Empty,
            rpc_tx_multicall_total = field::Empty,
            rpc_tx_multicall_logged = field::Empty,
            rpc_req_type = request_type.to_string()
        );
        let middleware_enter = span.enter();

        // trace event
        Span::with(|s| {
            s.rec_str("rpc_id", &request_id);
            s.rec_str("rpc_client", &client);
            s.rec_str("rpc_method", &method);
        });

        let (future_response, tracing_identifiers) = if method == "eth_sendRawTransaction" {
            drop(middleware_enter);
            match eth_send_raw_transaction(request, Arc::clone(&self.ctx), span) {
                Ok(result) => result,
                Err(err) => {
                    tracing::warn!(?err, "failed to parse eth_sendRawTransaction request");
                    let future: BoxFuture<'a, MethodResponse> = Box::pin(err.to_response_future(request_id.clone()));
                    (future, None)
                }
            }
        } else {
            let tracing_identifiers = match method.as_str() {
                "eth_call" | "eth_estimateGas" => TransactionTracingIdentifiers::from_call(request.params()).ok(),
                "eth_getTransactionByHash" | "eth_getTransactionReceipt" => TransactionTracingIdentifiers::from_transaction_query(request.params()).ok(),
                _ => None,
            };
            Span::with(|s| {
                if let Some(ref tx) = tracing_identifiers {
                    tx.record_span(s);
                }
            });
            // make span available to rpc-server
            drop(middleware_enter);
            request.extensions_mut().insert(span);
            let future: BoxFuture<'a, MethodResponse> = Box::pin(self.service.call(request));
            (future, tracing_identifiers)
        };

        let tx_ref = tracing_identifiers.as_ref();
        let multicall_ref = tx_ref.and_then(|tx| tx.multicall.as_ref());

        // track metrics
        #[cfg(feature = "metrics")]
        {
            // started requests
            metrics::inc_rpc_requests_started(&client, &method, tx_ref.map(|tx| tx.contract), tx_ref.map(|tx| tx.function), request_type);

            if let Some(tx) = tx_ref
                && let Some(multicall) = tx.multicall.as_ref()
            {
                multicall.record_rpc_requests_started(&client, &method, request_type);
            }
        }

        tracing::info!(
            rpc_client = %client,
            rpc_id = %request_id,
            rpc_method = %method,
            rpc_params = %request_params_str,
            rpc_tx_hash = %tx_ref.and_then(|tx| tx.hash).or_empty(),
            rpc_tx_contract = %tx_ref.map(|tx| tx.contract).or_empty(),
            rpc_tx_function = %tx_ref.map(|tx| tx.function).or_empty(),
            rpc_tx_from = %tx_ref.and_then(|tx| tx.from).or_empty(),
            rpc_tx_to = %tx_ref.and_then(|tx| tx.to).or_empty(),
            rpc_tx_multicall_total = %multicall_ref.map(|multicall| multicall.total_subcalls).or_empty(),
            rpc_tx_multicall_logged = %multicall_ref.map(|multicall| multicall.logged_subcalls_count()).or_empty(),
            rpc_tx_multicall_subcalls = %multicall_ref.map(|multicall| to_json_string(&multicall.logged_subcalls())).or_empty(),
            is_admin = %is_admin,
            "rpc request"
        );

        RpcResponse {
            client,
            id: request_id.to_string(),
            method: method.to_string(),
            tx: tracing_identifiers,
            start: Instant::now(),
            future_response,
        }
    }
}

/// Returns an error JSON-RPC response if the client is not allowed to perform the current operation.
fn reject_client<'a>(client: &RpcClientApp, id: Id<'_>) -> Option<BoxFuture<'a, MethodResponse>> {
    // reject unidentified clients when unknown clients are disabled
    if client.is_unknown() && !GlobalState::is_unknown_client_enabled() {
        return Some(Box::pin(StratusError::RPC(RpcError::ClientMissing).to_response_future(id)));
    }
    // reject explicitly blocked clients
    if GlobalState::is_client_blocked(client) {
        return Some(Box::pin(
            StratusError::RPC(RpcError::ClientBlocked { client: client.to_string() }).to_response_future(id),
        ));
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

            // only log rpc_result if log level is not info
            let rpc_result = if matches!(level, Level::INFO) { Default::default() } else { &response_result };
            let log_tracing_event = || {
                let tx_ref = resp.tx.as_ref();
                let multicall_ref = tx_ref.and_then(|tx| tx.multicall.as_ref());

                event_with!(
                    level,
                    rpc_client = %resp.client,
                    rpc_id = %resp.id,
                    rpc_method = %resp.method,
                    rpc_tx_hash = %tx_ref.and_then(|tx| tx.hash).or_empty(),
                    rpc_tx_contract = %tx_ref.map(|tx| tx.contract).or_empty(),
                    rpc_tx_function = %tx_ref.map(|tx| tx.function).or_empty(),
                    rpc_tx_from = %tx_ref.and_then(|tx| tx.from).or_empty(),
                    rpc_tx_to = %tx_ref.and_then(|tx| tx.to).or_empty(),
                    rpc_tx_multicall_total = %multicall_ref.map(|multicall| multicall.total_subcalls).or_empty(),
                    rpc_tx_multicall_logged = %multicall_ref.map(|multicall| multicall.logged_subcalls_count()).or_empty(),
                    rpc_tx_multicall_subcalls = %multicall_ref.map(|multicall| to_json_string(&multicall.logged_subcalls())).or_empty(),
                    %rpc_result,
                    rpc_success = %response_success,
                    duration_us = %elapsed.as_micros(),
                    "rpc response"
                );
            };

            sentry::configure_scope(|scope| {
                scope.set_user(Some(sentry::User {
                    username: Some(resp.client.to_string()),
                    ..Default::default()
                }));
            });

            log_tracing_event();

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

                if let Some(tx) = tx_ref
                    && let Some(multicall) = tx.multicall.as_ref()
                {
                    multicall.record_rpc_requests_finished(elapsed, resp.client, resp.method, rpc_result, error_code, response.is_success());
                }

                metrics::inc_rpc_response_size(response.as_json().get().len(), &*resp.client, resp.method.clone());
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

pub struct TransactionTracingIdentifiers {
    pub hash: Option<Hash>,
    pub contract: ContractName,
    pub function: SoliditySignature,
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub nonce: Option<Nonce>,
    pub multicall: Option<MulticallInfo>,
}

impl TransactionTracingIdentifiers {
    /// eth_sendRawTransaction
    pub fn from_transaction_input(input: &TransactionInput) -> anyhow::Result<Self> {
        Ok(Self {
            hash: Some(input.transaction_info.hash),
            contract: codegen::contract_name(&input.execution_info.to),
            function: codegen::function_sig(&input.execution_info.input),
            from: input.execution_info.signer.address(),
            to: input.execution_info.to,
            nonce: Some(input.execution_info.nonce),
            multicall: MulticallInfo::decode_opt(input.execution_info.to, &input.execution_info.input),
        })
    }

    /// eth_call / eth_estimateGas
    fn from_call(params: Params) -> anyhow::Result<Self> {
        let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;
        Ok(Self {
            hash: None,
            contract: codegen::contract_name(&call.to),
            function: codegen::function_sig(&call.data),
            from: call.from,
            to: call.to,
            nonce: None,
            multicall: MulticallInfo::decode_opt(call.to, &call.data),
        })
    }

    /// eth_getTransactionByHash / eth_getTransactionReceipt
    fn from_transaction_query(params: Params) -> anyhow::Result<Self> {
        let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
        Ok(Self {
            hash: Some(hash),
            contract: metrics::LABEL_MISSING,
            function: metrics::LABEL_MISSING,
            from: None,
            to: None,
            nonce: None,
            multicall: None,
        })
    }

    pub fn record_span(&self, span: Span) {
        span.rec_str("rpc_tx_contract", &self.contract);
        span.rec_str("rpc_tx_function", &self.function);
        if let Some(multicall) = &self.multicall {
            span.rec_str("rpc_tx_multicall_total", &multicall.total_subcalls);
            span.rec_str("rpc_tx_multicall_logged", &multicall.logged_subcalls_count());
        }

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
