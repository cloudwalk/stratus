//! Track RPC requests and responses using metrics and traces.

use std::future::Future;
#[cfg(feature = "metrics")]
#[cfg(feature = "metrics")]
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
// Active requests tracking
// -----------------------------------------------------------------------------
#[cfg(feature = "metrics")]
mod active_requests {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::RwLock;

    use lazy_static::lazy_static;

    use crate::eth::rpc::RpcClientApp;
    use crate::infra::metrics;

    lazy_static! {
        pub static ref COUNTERS: ActiveRequests = ActiveRequests::default();
    }

    #[derive(Default)]
    pub struct ActiveRequests {
        inner: RwLock<HashMap<String, Arc<AtomicU64>>>,
    }

    impl ActiveRequests {
        pub fn inc(&self, client: &RpcClientApp, method: &str) {
            let active = self.counter_for(client, method).fetch_add(1, Ordering::Relaxed) + 1;
            metrics::set_rpc_requests_active(active, client, method);
        }

        pub fn dec(&self, client: &RpcClientApp, method: &str) {
            let active = self.counter_for(client, method).fetch_sub(1, Ordering::Relaxed) - 1;
            metrics::set_rpc_requests_active(active, client, method);
        }

        fn counter_for(&self, client: &RpcClientApp, method: &str) -> Arc<AtomicU64> {
            let id = format!("{}::{}", client, method);

            // try to read counter
            let active_requests_read = self.inner.read().unwrap();
            if let Some(counter) = active_requests_read.get(&id) {
                return Arc::clone(counter);
            }
            drop(active_requests_read);

            // create a new counter
            let mut active_requests_write = self.inner.write().unwrap();
            let counter = Arc::new(AtomicU64::new(0));
            active_requests_write.insert(id.clone(), Arc::clone(&counter));
            counter
        }
    }
}

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
        let span = info_span!("rpc::request", cid = %new_cid(), request_client = field::Empty, request_id = field::Empty, request_method = field::Empty, request_function = field::Empty);
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
            s.rec_str("request_id", &request.id);
            s.rec_str("request_client", &client);
            s.rec_str("request_method", &method);
            s.rec_opt("request_function", &function);
        });

        // trace request
        tracing::info!(
            request_client = %client,
            request_id = %request.id,
            request_method = %method,
            request_function = %function.clone().unwrap_or_default(),
            request_params = ?request.params(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        {
            active_requests::COUNTERS.inc(&client, method);
            metrics::inc_rpc_requests_started(&client, method, function.clone());
        }

        drop(enter);
        RpcResponse {
            identifiers: RpcResponseIdentifiers {
                client,
                id: request.id.to_string(),
                method: method.to_string(),
                function,
            },
            start: Instant::now(),
            future_response: self.service.call(request),
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
    identifiers: RpcResponseIdentifiers,
    start: Instant,
    #[pin]
    future_response: ResponseFuture<BoxFuture<'a, MethodResponse>>,
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
                request_client = %proj.identifiers.client,
                request_id = %proj.identifiers.id,
                request_method = %proj.identifiers.method,
                request_function = %proj.identifiers.function.clone().unwrap_or_default(),
                request_duration_us = %elapsed.as_micros(),
                request_success = %response_success,
                request_result = %response_result,
                "rpc response"
            );

            // metrify response
            #[cfg(feature = "metrics")]
            {
                let mut rpc_result = "error";
                if response_success {
                    rpc_result = if_else!(response_result.contains("\"result\":null"), "missing", "present");
                }

                metrics::inc_rpc_requests_finished(
                    elapsed,
                    &proj.identifiers.client,
                    proj.identifiers.method.clone(),
                    proj.identifiers.function.clone(),
                    rpc_result,
                    response.is_success(),
                );
            }
        }

        response
    }
}

struct RpcResponseIdentifiers {
    client: RpcClientApp,
    id: String,
    method: String,
    function: Option<SoliditySignature>,
}

impl Drop for RpcResponseIdentifiers {
    fn drop(&mut self) {
        #[cfg(feature = "metrics")]
        {
            active_requests::COUNTERS.dec(&self.client, &self.method);
        }
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
