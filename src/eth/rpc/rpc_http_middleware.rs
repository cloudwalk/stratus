use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use futures::TryFutureExt;
use jsonrpsee::client_transport::ws::Uri;
use jsonrpsee::core::BoxError;
use jsonrpsee::server::HttpBody;
use jsonrpsee::server::HttpRequest;
use jsonrpsee::server::HttpResponse;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use tower::Service;

use crate::eth::rpc::RpcClientApp;
use crate::ext::not;

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
        let client_app = parse_client_app(request.headers(), request.uri());
        request.extensions_mut().insert(client_app);

        Box::pin(self.service.call(request).map_err(Into::into))
    }
}

/// Extracts the client application name from the `app` query parameter.
fn parse_client_app(headers: &HeaderMap<HeaderValue>, uri: &Uri) -> RpcClientApp {
    fn normalize(client_app: &str) -> String {
        client_app.trim().to_ascii_lowercase()
    }

    // try http headers
    for header in ["x-app", "x-stratus-app", "x-client", "x-stratus-client"] {
        if let Some(client_app) = headers.get(header) {
            let Ok(client_app) = client_app.to_str() else {
                tracing::warn!(%header, value = ?client_app, "failed to parse http header as ascii string");
                continue;
            };
            let client_app = normalize(client_app);
            if not(client_app.is_empty()) {
                return RpcClientApp::parse(&client_app);
            }
        }
    }

    // try query params
    let Some(query_params_str) = uri.query() else {
        return RpcClientApp::default();
    };
    let query_params: HashMap<String, String> = match serde_urlencoded::from_str(query_params_str) {
        Ok(url) => url,
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to parse http request query parameters");
            return RpcClientApp::Unknown;
        }
    };
    for param in ["app", "client"] {
        if let Some(client_app) = query_params.get(param) {
            let client_app = normalize(client_app);
            if not(client_app.is_empty()) {
                return RpcClientApp::parse(&client_app);
            }
        }
    }

    // not identified
    RpcClientApp::default()
}
