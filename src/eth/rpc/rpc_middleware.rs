//! Track RPC requests and responses using metrics and traces.

use std::future::Future;
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
use tracing::Span;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::next_rpc_param;
use crate::eth::rpc::parse_rpc_rlp;
use crate::eth::rpc::rpc_parser::RpcExtensionsExt;
use crate::eth::rpc::RpcClientApp;
use crate::ext::to_json_value;
use crate::if_else;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::new_cid;
use crate::infra::tracing::SpanExt;
use crate::infra::tracing::TracingExt;

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
            active_requests_write.insert(id, Arc::clone(&counter));
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
    type Future = RpcResponse<'a>;

    fn call(&self, mut request: jsonrpsee::types::Request<'a>) -> Self::Future {
        // track request
        let span = info_span!(
            "rpc::request",
            cid = %new_cid(),
            rpc_client = field::Empty,
            rpc_id = field::Empty,
            rpc_method = field::Empty,
            rpc_tx_hash = field::Empty,
            rpc_tx_from = field::Empty,
            rpc_tx_to = field::Empty,
            rpc_tx_nonce = field::Empty,
            rpc_tx_function = field::Empty
        );
        let enter = span.enter();

        // extract request data
        let client = request.extensions.rpc_client();
        let method = request.method_name().to_owned();
        let tx = match method.as_str() {
            "eth_call" | "eth_estimateGas" => TxTracingIdentifiers::from_call(request.params()).ok(),
            "eth_sendRawTransaction" => TxTracingIdentifiers::from_transaction(request.params()).ok(),
            "eth_getTransactionByHash" | "eth_getTransactionReceipt" => TxTracingIdentifiers::from_transaction_query(request.params()).ok(),
            _ => None,
        };

        // trace request
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
            rpc_params = %to_json_value(&request.params),
            rpc_tx_hash = %tx.as_ref().and_then(|tx|tx.hash).or_empty(),
            rpc_tx_function = %tx.as_ref().and_then(|tx|tx.function.clone()).or_empty(),
            rpc_tx_from = %tx.as_ref().and_then(|tx|tx.from).or_empty(),
            rpc_tx_to = %tx.as_ref().and_then(|tx|tx.to).or_empty(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        {
            active_requests::COUNTERS.inc(&client, &method);
            metrics::inc_rpc_requests_started(&client, &method, tx.as_ref().and_then(|tx| tx.function.clone()));
        }
        drop(enter);

        // make span available to rpc-server
        request.extensions_mut().insert(span);

        RpcResponse {
            identifiers: RpcResponseIdentifiers {
                client,
                id: request.id.to_string(),
                method: method.to_string(),
                tx,
            },
            start: Instant::now(),
            future_response: self.service.call(request),
        }
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
        let resp = self.project();
        let response = resp.future_response.poll(cx);

        // when ready, track response
        if let Poll::Ready(response) = &response {
            let elapsed = resp.start.elapsed();
            let _enter = response.extensions().enter_middleware_span();

            // trace response
            let response_success = response.is_success();
            let response_result = response.as_result();
            tracing::info!(
                rpc_client = %resp.identifiers.client,
                rpc_id = %resp.identifiers.id,
                rpc_method = %resp.identifiers.method,
                rpc_tx_hash = %resp.identifiers.tx.as_ref().and_then(|tx|tx.hash).or_empty(),
                rpc_tx_function = %resp.identifiers.tx.as_ref().and_then(|tx|tx.function.clone()).or_empty(),
                rpc_tx_from = %resp.identifiers.tx.as_ref().and_then(|tx|tx.from).or_empty(),
                rpc_tx_to = %resp.identifiers.tx.as_ref().and_then(|tx|tx.to).or_empty(),
                rpc_result = %response_result,
                rpc_success = %response_success,
                rpc_duration_us = %elapsed.as_micros(),
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
                    &resp.identifiers.client,
                    resp.identifiers.method.clone(),
                    resp.identifiers.tx.as_ref().and_then(|tx| tx.function.clone()),
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
    tx: Option<TxTracingIdentifiers>,
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

struct TxTracingIdentifiers {
    pub hash: Option<Hash>,
    pub function: Option<SoliditySignature>,
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub nonce: Option<Nonce>,
}

impl TxTracingIdentifiers {
    fn from_transaction(params: Params) -> anyhow::Result<Self> {
        let (_, data) = next_rpc_param::<Bytes>(params.sequence())?;
        let tx = parse_rpc_rlp::<TransactionInput>(&data)?;
        Ok(Self {
            hash: Some(tx.hash),
            function: tx.extract_function(),
            from: Some(tx.signer),
            to: tx.to,
            nonce: Some(tx.nonce),
        })
    }

    fn from_call(params: Params) -> anyhow::Result<Self> {
        let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;
        Ok(Self {
            hash: None,
            function: call.extract_function(),
            from: call.from,
            to: call.to,
            nonce: None,
        })
    }

    fn from_transaction_query(params: Params) -> anyhow::Result<Self> {
        let (_, hash) = next_rpc_param::<Hash>(params.sequence())?;
        Ok(Self {
            hash: Some(hash),
            function: None,
            from: None,
            to: None,
            nonce: None,
        })
    }

    pub fn record_span(&self, span: Span) {
        if let Some(tx_hash) = self.hash {
            span.rec_str("rpc_tx_hash", &tx_hash);
        }
        if let Some(ref tx_function) = self.function {
            span.rec_str("rpc_tx_function", &tx_function);
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
