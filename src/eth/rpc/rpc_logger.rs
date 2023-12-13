use jsonrpsee::helpers::MethodResponseResult;
use jsonrpsee::server::logger::HttpRequest;
use jsonrpsee::server::logger::Logger;
use jsonrpsee::server::logger::MethodKind;
use jsonrpsee::server::logger::TransportProtocol;
use jsonrpsee::types::Params;

#[derive(Clone, Debug)]
pub struct RpcLogger;

impl Logger for RpcLogger {
    type Instant = tokio::time::Instant;

    fn on_connect(&self, _: std::net::SocketAddr, _: &HttpRequest, _: TransportProtocol) {}

    fn on_request(&self, _: TransportProtocol) -> Self::Instant {
        Self::Instant::now()
    }

    fn on_call(&self, method: &str, _params: Params, _: MethodKind, _: TransportProtocol) {
        // let params = _params.as_str().unwrap_or_default();
        // tracing::info!(%method, %params, "rpc request");
        tracing::info!(%method, "rpc request");
    }

    fn on_result(&self, _: &str, _: MethodResponseResult, _: Self::Instant, _: TransportProtocol) {}

    fn on_response(&self, _: &str, _: Self::Instant, _: TransportProtocol) {}

    fn on_disconnect(&self, _: std::net::SocketAddr, _: TransportProtocol) {}
}
