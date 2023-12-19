use std::future::Future;
use std::task::Poll;
use std::time::Instant;

use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::MethodResponse;
use pin_project::pin_project;

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
        // track request trace
        tracing::info!(id = %request.id, method = %request.method, "rpc request");

        // track request metrics
        let labels = [("method", request.method.to_string())];
        metrics::inc_eth_rpc_request_started(&labels);

        RpcResponse {
            id: request.id.to_string(),
            method: request.method.to_string(),
            future_response: self.service.call(request),
            start: Instant::now(),
        }
    }
}

// -----------------------------------------------------------------------------
// Response handling
// -----------------------------------------------------------------------------

#[pin_project]
pub struct RpcResponse<F> {
    id: String,
    method: String,
    start: Instant,

    #[pin]
    future_response: F,
}

impl<F: Future<Output = MethodResponse>> Future for RpcResponse<F> {
    type Output = F::Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        // poll future
        let proj = self.project();
        let response = proj.future_response.poll(cx);

        // when ready, track execution metrics
        if let Poll::Ready(response) = &response {
            let elapsed = proj.start.elapsed();

            // track response trace
            tracing::info!(
                id = %proj.id,
                method = %proj.method,
                duration_ms = %elapsed.as_millis(),
                success = %response.success_or_error.is_success(),
                "rpc response"
            );

            // track response metrics
            let labels = [
                ("method", proj.method.to_string()),
                ("success", response.success_or_error.is_success().to_string())
            ];
            metrics::inc_eth_rpc_request_finished(elapsed, &labels);
        }

        response
    }
}
