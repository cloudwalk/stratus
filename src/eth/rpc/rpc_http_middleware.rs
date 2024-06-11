use core::future::Future;
use core::pin::Pin;
use std::collections::HashMap;

use futures::TryFutureExt;
use jsonrpsee::client_transport::ws::Uri;
use jsonrpsee::core::BoxError;
use jsonrpsee::server::HttpBody;
use jsonrpsee::server::HttpRequest;
use jsonrpsee::server::HttpResponse;
use tower::Service;

use crate::eth::rpc::RpcClientApp;

#[derive(Debug, Clone, derive_new::new)]
pub struct RpcHttpMiddleware<S> {
    service: S,
}

impl<S> Service<HttpRequest<HttpBody>> for RpcHttpMiddleware<S>
where
    S: Service<HttpRequest, Response = HttpResponse>,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut request: HttpRequest<HttpBody>) -> Self::Future {
        let client_app = parse_client_app(request.uri());
        request.extensions_mut().insert(client_app);

        Box::pin(self.service.call(request).map_err(Into::into))
    }
}

/// Extracts the client application name from the `app` query parameter.
fn parse_client_app(uri: &Uri) -> RpcClientApp {
    // parse query params
    let Some(query_params_str) = uri.query() else { return RpcClientApp::Unknown };
    let query_params: HashMap<String, String> = match serde_urlencoded::from_str(query_params_str) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!(reason = ?e, "failed to parse http request query parameters");
            return RpcClientApp::Unknown;
        }
    };

    // try to extract client from query params
    for param in ["app", "client"] {
        if let Some(client_app) = query_params.get(param) {
            return RpcClientApp::Identified(client_app.to_owned());
        }
    }
    RpcClientApp::Unknown
}
