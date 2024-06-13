//! Track RPC requests and responses using metrics and traces.
//!
//! TODO: If it becomes a bottleneck, it can be processed asynchronously.

use std::future::Future;
#[cfg(feature = "metrics")]
use std::sync::atomic::AtomicU64;
#[cfg(feature = "metrics")]
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Instant;

use futures::future::BoxFuture;
use jsonrpsee::server::middleware::rpc::layer::ResponseFuture;
use jsonrpsee::server::middleware::rpc::RpcService;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::Params;
use jsonrpsee::MethodResponse;
use pin_project::pin_project;
use tracing::field;
use tracing::info_span;
use tracing::instrument::Instrumented;
use tracing::Instrument;
use tracing::Span;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::RpcClientApp;
use crate::ext::SpanExt;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::new_cid;

// -----------------------------------------------------------------------------
// Global metrics
// -----------------------------------------------------------------------------
#[cfg(feature = "metrics")]
static ACTIVE_REQUESTS: AtomicU64 = AtomicU64::new(0);

// -----------------------------------------------------------------------------
// Request handling
// -----------------------------------------------------------------------------

#[derive(Debug, derive_new::new)]
pub struct RpcMiddleware {
    service: RpcService,
}

impl<'a> RpcServiceT<'a> for RpcMiddleware {
    type Future = Instrumented<RpcResponse<'a>>;

    fn call(&self, request: jsonrpsee::types::Request<'a>) -> Self::Future {
        // track request
        let span = info_span!("rpc::request", cid = %new_cid(), client = field::Empty, method = field::Empty, function = field::Empty);
        let enter = span.enter();

        // extract request data
        let client = extract_client_app(&request);
        let method = request.method_name();
        let function = match method {
            "eth_call" | "eth_estimateGas" => extract_call_function(request.params()),
            "eth_sendRawTransaction" => extract_transaction_function(request.params()),
            _ => None,
        };
        Span::with(|s| {
            s.rec_str("client", &client);
            s.rec_str("method", &method);
            s.rec_opt("function", &function);
        });

        // trace request
        tracing::info!(
            client = %client,
            id = %request.id,
            method = %method,
            function = %function.clone().unwrap_or_default(),
            params = ?request.params(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        {
            let active = ACTIVE_REQUESTS.fetch_add(1, Ordering::Relaxed) + 1;
            metrics::set_rpc_requests_active(active, &client, method, function.clone());
            metrics::inc_rpc_requests_started(&client, method, function.clone());
        }

        drop(enter);
        RpcResponse {
            client,
            id: request.id.to_string(),
            method: method.to_string(),
            function,
            future_response: self.service.call(request),
            start: Instant::now(),
        }
        .instrument(span)
    }
}

// -----------------------------------------------------------------------------
// Response handling
// -----------------------------------------------------------------------------

/// https://blog.adamchalmers.com/pin-unpin/
#[pin_project]
pub struct RpcResponse<'a> {
    #[pin]
    future_response: ResponseFuture<BoxFuture<'a, MethodResponse>>,

    // request metadata
    client: RpcClientApp,
    id: String,
    method: String,
    function: Option<SoliditySignature>,

    // request services
    start: Instant,
}

impl<'a> Future for RpcResponse<'a> {
    type Output = MethodResponse;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        // poll future
        let proj = self.project();
        let response = proj.future_response.poll(cx);

        // when ready, track response
        if let Poll::Ready(response) = &response {
            let elapsed = proj.start.elapsed();

            // trace response
            let response_success = response.is_success();
            let response_result = response.as_result();
            tracing::info!(
                client = %proj.client,
                id = %proj.id,
                method = %proj.method,
                function = %proj.function.clone().unwrap_or_default(),
                duration_us = %elapsed.as_micros(),
                success = %response_success,
                result = %response_result,
                "rpc response"
            );

            // metrify response
            #[cfg(feature = "metrics")]
            {
                let active = ACTIVE_REQUESTS.fetch_sub(1, Ordering::Relaxed) - 1;
                metrics::set_rpc_requests_active(active, &*proj.client, proj.method.clone(), proj.function.clone());

                let mut rpc_result = "error";
                if response_success {
                    rpc_result = if_else!(response_result.contains("\"result\":null"), "missing", "present");
                }

                metrics::inc_rpc_requests_finished(
                    elapsed,
                    &*proj.client,
                    proj.method.clone(),
                    proj.function.clone(),
                    rpc_result,
                    response.is_success(),
                );
            }
        }

        response
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn extract_client_app(request: &jsonrpsee::types::Request) -> RpcClientApp {
    request.extensions().get::<RpcClientApp>().unwrap_or(&RpcClientApp::Unknown).clone()
}

fn extract_call_function(params: Params) -> Option<SoliditySignature> {
    let (_, call) = next_rpc_param::<CallInput>(params.sequence()).ok()?;
    call.extract_function()
}

fn extract_transaction_function(params: Params) -> Option<SoliditySignature> {
    let (_, data) = next_rpc_param::<Bytes>(params.sequence()).ok()?;
    let transaction = parse_rpc_rlp::<TransactionInput>(&data).ok()?;
    transaction.extract_function()
}
