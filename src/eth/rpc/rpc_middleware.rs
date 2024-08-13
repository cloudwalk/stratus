//! Track RPC requests and responses using metrics and traces.

use std::future::Future;
use std::task::Poll;
use std::time::Instant;

use futures::future::BoxFuture;
use jsonrpsee::server::middleware::rpc::layer::ResponseFuture;
use jsonrpsee::server::middleware::rpc::RpcService;
use jsonrpsee::server::middleware::rpc::RpcServiceT;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::Params;
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
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
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

// -----------------------------------------------------------------------------
// Request handling
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct RpcMiddleware {
    service: RpcService,
}

impl RpcMiddleware {
    pub fn new(service: RpcService) -> Self {
        Self { service }
    }
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
            rpc_tx_contract = field::Empty,
            rpc_tx_function = field::Empty
        );
        let middleware_enter = span.enter();

        // extract request data
        let mut client = request.extensions.rpc_client();
        let method = request.method_name().to_owned();
        let tx = match method.as_str() {
            "eth_call" | "eth_estimateGas" => TransactionTracingIdentifiers::from_call(request.params()).ok(),
            "eth_sendRawTransaction" => TransactionTracingIdentifiers::from_transaction(request.params()).ok(),
            "eth_getTransactionByHash" | "eth_getTransactionReceipt" => TransactionTracingIdentifiers::from_transaction_query(request.params()).ok(),
            _ => None,
        };
        if let Some(tx_client) = tx.as_ref().and_then(|tx| tx.client.clone()) {
            request.extensions_mut().insert(tx_client.clone());
            client = tx_client;
        }

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
            rpc_params = %to_json_string(&request.params),
            rpc_tx_hash = %tx.as_ref().and_then(|tx|tx.hash).or_empty(),
            rpc_tx_contract = %tx.as_ref().map(|tx|tx.contract).or_empty(),
            rpc_tx_function = %tx.as_ref().map(|tx|tx.function).or_empty(),
            rpc_tx_from = %tx.as_ref().and_then(|tx|tx.from).or_empty(),
            rpc_tx_to = %tx.as_ref().and_then(|tx|tx.to).or_empty(),
            "rpc request"
        );

        // metrify request
        #[cfg(feature = "metrics")]
        {
            let tx_ref = tx.as_ref();
            metrics::inc_rpc_requests_started(&client, &method, tx_ref.map(|tx| tx.contract), tx_ref.map(|tx| tx.function));
        }
        drop(middleware_enter);

        // make span available to rpc-server
        request.extensions_mut().insert(span);

        RpcResponse {
            client,
            id: request.id.to_string(),
            method: method.to_string(),
            tx,
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
    // identifiers
    client: RpcClientApp,
    id: String,
    method: String,
    tx: Option<TransactionTracingIdentifiers>,

    // data
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
            let _middleware_enter = response.extensions().enter_middleware_span();

            // trace response
            let response_success = response.is_success();
            let response_result: JsonValue = from_json_str(response.as_result());

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

            // metrify response
            #[cfg(feature = "metrics")]
            {
                let rpc_result = match response_result.get("result") {
                    Some(result) => if_else!(result.is_null(), metrics::LABEL_MISSING, metrics::LABEL_PRESENT),
                    None => metrics::LABEL_ERROR,
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
    // eth_sendRawTransaction
    fn from_transaction(params: Params) -> anyhow::Result<Self> {
        let (params, tx_data) = next_rpc_param::<Bytes>(params.sequence())?;
        let tx = parse_rpc_rlp::<TransactionInput>(&tx_data)?;
        let client = next_rpc_param::<RpcClientApp>(params);

        Ok(Self {
            client: client.map(|(_, client)| client).ok(),
            hash: Some(tx.hash),
            contract: codegen::contract_name_for_o11y(&tx.to),
            function: codegen::function_sig_for_o11y(&tx.input),
            from: Some(tx.signer),
            to: tx.to,
            nonce: Some(tx.nonce),
        })
    }

    /// eth_call / eth_estimateGas
    fn from_call(params: Params) -> anyhow::Result<Self> {
        let (_, call) = next_rpc_param::<CallInput>(params.sequence())?;
        Ok(Self {
            client: None,
            hash: None,
            contract: codegen::contract_name_for_o11y(&call.to),
            function: codegen::function_sig_for_o11y(&call.data),
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
