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

use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::Params;
use jsonrpsee::MethodResponse;
use pin_project::pin_project;

use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

// -----------------------------------------------------------------------------
// Global metrics
// -----------------------------------------------------------------------------
#[cfg(feature = "metrics")]
static ACTIVE_REQUESTS: AtomicU64 = AtomicU64::new(0);

// -----------------------------------------------------------------------------
// Request handling
// -----------------------------------------------------------------------------

#[derive(Debug, derive_new::new)]
pub struct RpcMiddleware<S> {
    service: S,
}

impl<'a, S> RpcServiceT<'a> for RpcMiddleware<S>
where
    S: RpcServiceT<'a> + Send + Sync,
{
    type Future = RpcResponse<S::Future>;

    fn call(&self, request: jsonrpsee::types::Request<'a>) -> Self::Future {
        // extract signature if available
        let method = request.method_name();
        let function = match method {
            "eth_call" | "eth_estimateGas" => extract_function_from_call(request.params()),
            "eth_sendRawTransaction" => extract_function_from_transaction(request.params()),
            _ => None,
        };

        // trace request
        tracing::info!(
            id = %request.id,
            %method,
            function = %function.clone().unwrap_or_default(),
            params = ?request.params(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        {
            let active = ACTIVE_REQUESTS.fetch_add(1, Ordering::Relaxed) + 1;
            metrics::set_rpc_requests_active(active, method, function.clone());
            metrics::inc_rpc_requests_started(method, function.clone());
        }

        RpcResponse {
            id: request.id.to_string(),
            method: method.to_string(),
            function,
            future_response: self.service.call(request),
            start: Instant::now(),
        }
    }
}

fn extract_function_from_call(params: Params) -> Option<SoliditySignature> {
    let (_, call) = next_rpc_param::<CallInput>(params.sequence()).ok()?;
    call.extract_function()
}

fn extract_function_from_transaction(params: Params) -> Option<SoliditySignature> {
    let (_, data) = next_rpc_param::<Bytes>(params.sequence()).ok()?;
    let transaction = parse_rpc_rlp::<TransactionInput>(&data).ok()?;
    transaction.extract_function()
}

// -----------------------------------------------------------------------------
// Response handling
// -----------------------------------------------------------------------------

/// https://blog.adamchalmers.com/pin-unpin/
#[pin_project]
pub struct RpcResponse<F> {
    #[pin]
    future_response: F,

    id: String,
    method: String,
    function: Option<SoliditySignature>,
    start: Instant,
}

impl<F: Future<Output = MethodResponse>> Future for RpcResponse<F> {
    type Output = F::Output;

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
                metrics::set_rpc_requests_active(active, proj.method.clone(), proj.function.clone());

                let mut rpc_result = "error";
                if response_success {
                    rpc_result = if_else!(response_result.contains("\"result\":null"), "missing", "present");
                }
                metrics::inc_rpc_requests_finished(elapsed, proj.method.clone(), proj.function.clone(), rpc_result, response.is_success());
            }
        }

        response
    }
}
