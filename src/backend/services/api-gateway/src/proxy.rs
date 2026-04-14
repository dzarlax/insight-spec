//! Reverse proxy module — routes requests to upstream services.
//!
//! Reads route definitions from YAML config and registers catch-all
//! handlers that forward requests (with auth headers) to upstream services.
//!
//! Routes marked `public: true` skip authentication.
//! Routes with `public: false` (default) require a valid JWT — the gateway's
//! auth middleware validates the token before the request reaches the proxy.
//!
//! # Configuration
//!
//! ```yaml
//! modules:
//!   proxy:
//!     config:
//!       routes:
//!         - prefix: "/analytics"
//!           upstream: "http://analytics-api:8081"
//!         - prefix: "/public-api"
//!           upstream: "http://public-service:8080"
//!           public: true
//! ```

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use modkit::api::{OpenApiRegistry, OperationBuilder};
use modkit::contracts::{Module, RestApiCapability};
use modkit::context::ModuleCtx;
use serde::Deserialize;

/// Route definition: prefix → upstream.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    /// URL path prefix to match (e.g. "/analytics").
    pub prefix: String,
    /// Upstream service base URL (e.g. "http://analytics-api:8081").
    pub upstream: String,
    /// If true, this route does NOT require authentication.
    /// Default: false (all routes require auth).
    #[serde(default)]
    pub public: bool,
}

/// Module configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub routes: Vec<RouteConfig>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self { routes: vec![] }
    }
}

/// Shared state for proxy handlers.
#[derive(Debug, Clone)]
struct ProxyState {
    client: reqwest::Client,
    upstream: String,
    prefix: String,
}

/// Reverse proxy module.
#[modkit::module(
    name = "proxy",
    capabilities = [rest]
)]
pub struct ProxyModule {
    config: OnceLock<Arc<ProxyConfig>>,
}

impl Default for ProxyModule {
    fn default() -> Self {
        Self {
            config: OnceLock::new(),
        }
    }
}

#[async_trait]
impl Module for ProxyModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        let config: ProxyConfig = ctx.config()?;

        for route in &config.routes {
            tracing::info!(
                prefix = %route.prefix,
                upstream = %route.upstream,
                public = route.public,
                "proxy route configured"
            );
        }

        if config.routes.is_empty() {
            tracing::warn!("proxy: no routes configured — module will be inactive");
        }

        self.config
            .set(Arc::new(config))
            .map_err(|_| anyhow::anyhow!("proxy module already initialized"))?;

        Ok(())
    }
}

impl RestApiCapability for ProxyModule {
    fn register_rest(
        &self,
        _ctx: &ModuleCtx,
        mut router: Router,
        openapi: &dyn OpenApiRegistry,
    ) -> anyhow::Result<Router> {
        let config = self
            .config
            .get()
            .ok_or_else(|| anyhow::anyhow!("proxy module not initialized"))?;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        for route in &config.routes {
            let prefix = route.prefix.trim_end_matches('/').to_owned();
            let upstream = route.upstream.trim_end_matches('/').to_owned();

            let state = Arc::new(ProxyState {
                client: client.clone(),
                upstream,
                prefix: prefix.clone(),
            });

            let wildcard_path = format!("{prefix}/{{*rest}}");
            let desc = format!("Proxy → {}", route.upstream);

            let methods = [Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH];

            for method in methods {
                let st = state.clone();
                let handler = move |req: Request<Body>| {
                    let st = st.clone();
                    async move { proxy_handler(st, req).await }
                };

                router = if route.public {
                    register_public(
                        router, openapi, method, &wildcard_path, &desc, handler,
                    )
                } else {
                    register_authenticated(
                        router, openapi, method, &wildcard_path, &desc, handler,
                    )
                };
            }

            tracing::info!(
                path = %wildcard_path,
                public = route.public,
                "registered proxy route"
            );
        }

        Ok(router)
    }
}

/// Register an authenticated proxy route via `OperationBuilder`.
fn register_authenticated<H, T>(
    router: Router,
    openapi: &dyn OpenApiRegistry,
    method: Method,
    path: &str,
    summary: &str,
    handler: H,
) -> Router
where
    H: axum::handler::Handler<T, ()> + Clone,
    T: 'static,
{
    OperationBuilder::new(method, path)
        .summary(summary)
        .authenticated()
        .no_license_required()
        .json_response(StatusCode::OK, "Proxied response")
        .handler(handler)
        .register(router, openapi)
}

/// Register a public (no auth) proxy route via `OperationBuilder`.
fn register_public<H, T>(
    router: Router,
    openapi: &dyn OpenApiRegistry,
    method: Method,
    path: &str,
    summary: &str,
    handler: H,
) -> Router
where
    H: axum::handler::Handler<T, ()> + Clone,
    T: 'static,
{
    OperationBuilder::new(method, path)
        .summary(summary)
        .public()
        .json_response(StatusCode::OK, "Proxied response")
        .handler(handler)
        .register(router, openapi)
}

/// Forward the request to the upstream service.
async fn proxy_handler(state: Arc<ProxyState>, req: Request<Body>) -> Response {
    match forward_request(&state, req).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(
                upstream = %state.upstream,
                error = %e,
                "proxy request failed"
            );
            (
                StatusCode::BAD_GATEWAY,
                serde_json::json!({
                    "type": "urn:insight:error:bad_gateway",
                    "title": "Bad Gateway",
                    "status": 502,
                    "detail": format!("upstream request failed: {e}")
                })
                .to_string(),
            )
                .into_response()
        }
    }
}

/// Build and send the proxied request.
async fn forward_request(
    state: &ProxyState,
    req: Request<Body>,
) -> Result<Response, anyhow::Error> {
    let (parts, body) = req.into_parts();

    // Strip the proxy prefix from the path to get the upstream path.
    // e.g. /analytics/v1/metrics → /v1/metrics
    let original_path = parts.uri.path();
    let upstream_path = original_path
        .strip_prefix(&state.prefix)
        .unwrap_or(original_path);

    // Build upstream URI
    let upstream_uri = if let Some(query) = parts.uri.query() {
        format!("{}{upstream_path}?{query}", state.upstream)
    } else {
        format!("{}{upstream_path}", state.upstream)
    };

    // Read request body (max 16 MB)
    let body_bytes = axum::body::to_bytes(body, 16 * 1024 * 1024).await?;

    // Build proxied request — forward method, headers, and body
    let mut upstream_req = state.client.request(parts.method.clone(), &upstream_uri);

    for (name, value) in &parts.headers {
        if name != "host" {
            upstream_req = upstream_req.header(name, value);
        }
    }

    let upstream_resp = upstream_req.body(body_bytes).send().await?;

    // Convert reqwest response back to axum response
    let status = StatusCode::from_u16(upstream_resp.status().as_u16())?;
    let mut builder = Response::builder().status(status);

    for (name, value) in upstream_resp.headers() {
        builder = builder.header(name, value);
    }

    let resp_headers = upstream_resp.headers().clone();
    let resp_body = upstream_resp.bytes().await?;

    let mut response = builder.body(Body::from(resp_body))?;

    if !resp_headers.contains_key("content-type") {
        response.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("application/json"),
        );
    }

    Ok(response)
}
