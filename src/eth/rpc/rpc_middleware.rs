//! Track RPC requests and responses using metrics and traces.
//!
//! TODO: If it becomes a bottleneck, it can be processed asynchronously.

use std::future::Future;
use std::task::Poll;
use std::time::Instant;

use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::Params;
use jsonrpsee::MethodResponse;
use pin_project::pin_project;

use crate::eth::codegen::{self};
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Signature4Bytes;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

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
            function = %function.unwrap_or_default(),
            // params = ?request.params(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        metrics::inc_rpc_requests_started(method, function);

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
    let data = call.data;
    extract_function_signature(data.get(..4)?.try_into().ok()?)
}

fn extract_function_from_transaction(params: Params) -> Option<SoliditySignature> {
    let (_, data) = next_rpc_param::<Bytes>(params.sequence()).ok()?;
    let transaction = parse_rpc_rlp::<TransactionInput>(&data).ok()?;
    if transaction.is_contract_deployment() {
        return Some("contract_deployment");
    }
    extract_function_signature(transaction.input.get(..4)?.try_into().ok()?)
}

fn extract_function_signature(id: Signature4Bytes) -> Option<SoliditySignature> {
    match codegen::SIGNATURES_4_BYTES.get(&id) {
        Some(signature) => Some(signature),
        None => Some("unknown"),
    }
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
            tracing::info!(
                id = %proj.id,
                method = %proj.method,
                function = %proj.function.unwrap_or_default(),
                duration_ms = %elapsed.as_millis(),
                success = %response.is_success(),
                // result = %response.result,
                "rpc response"
            );

            // metrify response
            #[cfg(feature = "metrics")]
            metrics::inc_rpc_requests_finished(elapsed, proj.method.clone(), *proj.function, response.success_or_error.is_success());
        }

        response
    }
}
