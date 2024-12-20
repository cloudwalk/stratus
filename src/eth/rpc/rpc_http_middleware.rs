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

use crate::eth::primitives::StratusError;
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
        let authentication = parse_admin_password(request.headers());
        request.extensions_mut().insert(client_app);
        request.extensions_mut().insert(authentication);

        Box::pin(self.service.call(request).map_err(Into::into))
    }
}

#[derive(Debug, Clone)]
pub enum Authentication {
    Admin,
    None,
}

impl Authentication {
    pub fn auth_admin(&self) -> Result<(), StratusError> {
        if matches!(self, Authentication::Admin) {
            return Ok(())
        }
        Err(StratusError::InvalidPassword)
    }
}

/// Checks if the provided admin password is correct
fn parse_admin_password(headers: &HeaderMap<HeaderValue>) -> Authentication {
    let Ok(real_pass) = std::env::var("ADMIN_PASSWORD") else {
        return Authentication::Admin;
    };
    if real_pass.is_empty() {
        return Authentication::Admin;
    }
    let Some(value) = headers.get("Authorization") else {
        return Authentication::None;
    };
    let Ok(value) = value.to_str() else { return Authentication::None };
    let Some(password) = value.strip_prefix("Password ") else {
        return Authentication::None;
    };
    if real_pass == password {
        Authentication::Admin
    } else {
        Authentication::None
    }
}

/// Extracts the client application name from the `app` query parameter.
fn parse_client_app(headers: &HeaderMap<HeaderValue>, uri: &Uri) -> RpcClientApp {
    fn try_query_params(uri: &Uri) -> Option<RpcClientApp> {
        let query_params = uri.query()?;
        let query_params: HashMap<String, String> = match serde_urlencoded::from_str(query_params) {
            Ok(url) => url,
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to parse http request query parameters");
                return None;
            }
        };
        for param in ["app", "client"] {
            if let Some(name) = query_params.get(param) {
                return Some(RpcClientApp::parse(name));
            }
        }
        None
    }

    fn try_headers(headers: &HeaderMap<HeaderValue>) -> Option<RpcClientApp> {
        for header in ["x-app", "x-stratus-app", "x-client", "x-stratus-client"] {
            if let Some(name) = headers.get(header) {
                let Ok(name) = name.to_str() else {
                    tracing::warn!(%header, value = ?name, "failed to parse http header as ascii string");
                    continue;
                };
                if not(name.is_empty()) {
                    return Some(RpcClientApp::parse(name));
                }
            }
        }
        None
    }

    fn try_path(uri: &Uri) -> Option<RpcClientApp> {
        let path = uri.path();
        let mut path_parts = path.split('/');
        while let Some(part) = path_parts.next() {
            if part == "app" || part == "client" {
                match path_parts.next() {
                    Some(name) => return Some(RpcClientApp::parse(name)),
                    None => return None,
                }
            }
        }
        None
    }

    if let Some(app) = try_query_params(uri) {
        return app;
    }
    if let Some(app) = try_headers(headers) {
        return app;
    }
    if let Some(app) = try_path(uri) {
        return app;
    }
    RpcClientApp::Unknown
}
