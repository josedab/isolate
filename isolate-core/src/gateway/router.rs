//! Gateway router and configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Delete => write!(f, "DELETE"),
            Self::Patch => write!(f, "PATCH"),
            Self::Options => write!(f, "OPTIONS"),
            Self::Head => write!(f, "HEAD"),
        }
    }
}

/// A route handler identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RouteHandler {
    /// Create a sandbox.
    CreateSandbox,
    /// Get sandbox status.
    GetSandbox,
    /// Run a sandbox.
    RunSandbox,
    /// Terminate a sandbox.
    TerminateSandbox,
    /// List sandboxes.
    ListSandboxes,
    /// Get metrics.
    GetMetrics,
    /// Health check.
    HealthCheck,
    /// Stream sandbox output via SSE.
    StreamOutput,
    /// Custom handler.
    Custom(String),
}

/// A route definition.
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method.
    pub method: HttpMethod,
    /// URL path pattern (e.g., "/api/v1/sandboxes/:id").
    pub path: String,
    /// Handler for this route.
    pub handler: RouteHandler,
    /// Whether this route requires authentication.
    pub requires_auth: bool,
    /// Rate limit group (routes in same group share limits).
    pub rate_limit_group: Option<String>,
}

impl Route {
    /// Create a new route.
    pub fn new(method: HttpMethod, path: impl Into<String>, handler: RouteHandler) -> Self {
        Self { method, path: path.into(), handler, requires_auth: false, rate_limit_group: None }
    }

    /// Set whether authentication is required.
    pub fn with_auth(mut self, required: bool) -> Self {
        self.requires_auth = required;
        self
    }

    /// Set the rate limit group.
    pub fn with_rate_limit_group(mut self, group: impl Into<String>) -> Self {
        self.rate_limit_group = Some(group.into());
        self
    }

    /// Check if a request path matches this route, extracting path parameters.
    pub fn matches(&self, method: &HttpMethod, path: &str) -> Option<HashMap<String, String>> {
        if &self.method != method {
            return None;
        }

        let route_parts: Vec<&str> = self.path.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        if route_parts.len() != path_parts.len() {
            return None;
        }

        let mut params = HashMap::new();

        for (route_part, path_part) in route_parts.iter().zip(path_parts.iter()) {
            if route_part.starts_with(':') {
                let param_name = &route_part[1..];
                params.insert(param_name.to_string(), path_part.to_string());
            } else if route_part != path_part {
                return None;
            }
        }

        Some(params)
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per window.
    pub max_requests: u32,
    /// Window duration.
    pub window: Duration,
    /// Whether to include rate limit headers in responses.
    pub include_headers: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self { max_requests: 100, window: Duration::from_secs(60), include_headers: true }
    }
}

/// Per-client rate limit state.
struct RateLimitState {
    request_count: u32,
    window_start: Instant,
}

/// Rate limiter.
pub struct RateLimiter {
    config: RateLimitConfig,
    clients: Arc<RwLock<HashMap<String, RateLimitState>>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        Self { config, clients: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Check if a request from the given client is allowed.
    pub fn check(&self, client_id: &str) -> RateLimitResult {
        let mut clients = self.clients.write();
        let now = Instant::now();

        let state = clients
            .entry(client_id.to_string())
            .or_insert(RateLimitState { request_count: 0, window_start: now });

        // Reset window if expired
        if now.duration_since(state.window_start) >= self.config.window {
            state.request_count = 0;
            state.window_start = now;
        }

        state.request_count += 1;

        if state.request_count > self.config.max_requests {
            let retry_after =
                self.config.window.saturating_sub(now.duration_since(state.window_start));
            RateLimitResult::Limited { retry_after, limit: self.config.max_requests, remaining: 0 }
        } else {
            RateLimitResult::Allowed {
                limit: self.config.max_requests,
                remaining: self.config.max_requests - state.request_count,
            }
        }
    }

    /// Clean up expired client entries.
    pub fn cleanup(&self) {
        let mut clients = self.clients.write();
        let now = Instant::now();
        clients.retain(|_, state| now.duration_since(state.window_start) < self.config.window * 2);
    }
}

/// Result of a rate limit check.
#[derive(Debug)]
pub enum RateLimitResult {
    /// Request is allowed.
    Allowed {
        /// Maximum requests per window.
        limit: u32,
        /// Remaining requests in this window.
        remaining: u32,
    },
    /// Request is rate-limited.
    Limited {
        /// Duration until the client can retry.
        retry_after: Duration,
        /// Maximum requests per window.
        limit: u32,
        /// Remaining requests (always 0).
        remaining: u32,
    },
}

impl RateLimitResult {
    /// Check if the request is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }
}

/// Gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// API prefix (e.g., "/api/v1").
    pub prefix: String,
    /// Listen address.
    pub listen_addr: String,
    /// Rate limiting configuration.
    pub rate_limit: RateLimitConfig,
    /// API keys for authentication (key -> name).
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
    /// CORS allowed origins.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Maximum request body size in bytes.
    pub max_body_size: usize,
    /// Request timeout.
    pub request_timeout: Duration,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            prefix: "/api/v1".to_string(),
            listen_addr: "0.0.0.0:8080".to_string(),
            rate_limit: RateLimitConfig::default(),
            api_keys: HashMap::new(),
            cors_origins: vec!["*".to_string()],
            max_body_size: 50 * 1024 * 1024, // 50 MB
            request_timeout: Duration::from_secs(300),
        }
    }
}

/// The HTTP/REST gateway router.
pub struct GatewayRouter {
    /// Configuration.
    config: GatewayConfig,
    /// Registered routes.
    routes: Vec<Route>,
    /// Rate limiter.
    rate_limiter: RateLimiter,
}

impl GatewayRouter {
    /// Create a new gateway router.
    pub fn new(config: GatewayConfig) -> Self {
        let rate_limiter = RateLimiter::new(config.rate_limit.clone());
        let mut router = Self { config, routes: Vec::new(), rate_limiter };
        router.register_default_routes();
        router
    }

    /// Register the default API routes.
    fn register_default_routes(&mut self) {
        let prefix = self.config.prefix.clone();

        self.routes.push(
            Route::new(
                HttpMethod::Post,
                format!("{}/sandboxes", prefix),
                RouteHandler::CreateSandbox,
            )
            .with_auth(true)
            .with_rate_limit_group("write"),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Get,
                format!("{}/sandboxes/:id", prefix),
                RouteHandler::GetSandbox,
            )
            .with_auth(true),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Post,
                format!("{}/sandboxes/:id/run", prefix),
                RouteHandler::RunSandbox,
            )
            .with_auth(true)
            .with_rate_limit_group("execution"),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Delete,
                format!("{}/sandboxes/:id", prefix),
                RouteHandler::TerminateSandbox,
            )
            .with_auth(true),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Get,
                format!("{}/sandboxes", prefix),
                RouteHandler::ListSandboxes,
            )
            .with_auth(true),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Get,
                format!("{}/sandboxes/:id/stream", prefix),
                RouteHandler::StreamOutput,
            )
            .with_auth(true),
        );

        self.routes.push(
            Route::new(
                HttpMethod::Get,
                format!("{}/metrics", prefix),
                RouteHandler::GetMetrics,
            )
            .with_auth(true),
        );

        self.routes.push(Route::new(
            HttpMethod::Get,
            format!("{}/health", prefix),
            RouteHandler::HealthCheck,
        ));
    }

    /// Add a custom route.
    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
    }

    /// Find the matching route for a request.
    pub fn match_route(
        &self,
        method: &HttpMethod,
        path: &str,
    ) -> Option<(&Route, HashMap<String, String>)> {
        for route in &self.routes {
            if let Some(params) = route.matches(method, path) {
                return Some((route, params));
            }
        }
        None
    }

    /// Check rate limit for a client.
    pub fn check_rate_limit(&self, client_id: &str) -> RateLimitResult {
        self.rate_limiter.check(client_id)
    }

    /// Authenticate a request using API key.
    pub fn authenticate(&self, api_key: &str) -> Option<String> {
        self.config.api_keys.get(api_key).cloned()
    }

    /// Get all registered routes.
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Get the configuration.
    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    /// Generate a simplified OpenAPI-style spec.
    pub fn openapi_spec(&self) -> serde_json::Value {
        let mut paths = serde_json::Map::new();

        for route in &self.routes {
            let method_str = route.method.to_string().to_lowercase();
            let entry = paths
                .entry(route.path.clone())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

            if let serde_json::Value::Object(methods) = entry {
                let mut operation = serde_json::Map::new();
                operation.insert(
                    "summary".to_string(),
                    serde_json::Value::String(format!("{:?}", route.handler)),
                );
                operation
                    .insert("security".to_string(), serde_json::Value::Bool(route.requires_auth));

                // Extract path parameters
                let params: Vec<serde_json::Value> = route
                    .path
                    .split('/')
                    .filter(|p| p.starts_with(':'))
                    .map(|p| {
                        serde_json::json!({
                            "name": &p[1..],
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"}
                        })
                    })
                    .collect();

                if !params.is_empty() {
                    operation.insert("parameters".to_string(), serde_json::Value::Array(params));
                }

                methods.insert(method_str, serde_json::Value::Object(operation));
            }
        }

        serde_json::json!({
            "openapi": "3.0.3",
            "info": {
                "title": "Isolate Sandbox API",
                "version": "1.0.0",
                "description": "REST API for managing Isolate sandboxes"
            },
            "paths": paths
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_matching() {
        let route = Route::new(HttpMethod::Get, "/api/v1/sandboxes/:id", RouteHandler::GetSandbox);

        let params = route.matches(&HttpMethod::Get, "/api/v1/sandboxes/abc-123").unwrap();
        assert_eq!(params.get("id"), Some(&"abc-123".to_string()));

        // Wrong method
        assert!(route.matches(&HttpMethod::Post, "/api/v1/sandboxes/abc-123").is_none());

        // Wrong path
        assert!(route.matches(&HttpMethod::Get, "/api/v1/sandboxes").is_none());
    }

    #[test]
    fn test_route_no_params() {
        let route = Route::new(HttpMethod::Get, "/api/v1/health", RouteHandler::HealthCheck);

        let params = route.matches(&HttpMethod::Get, "/api/v1/health").unwrap();
        assert!(params.is_empty());
    }

    #[test]
    fn test_gateway_router_default_routes() {
        let router = GatewayRouter::new(GatewayConfig::default());
        assert!(router.routes().len() >= 8);

        // Test route matching
        let (route, params) = router.match_route(&HttpMethod::Get, "/api/v1/health").unwrap();
        assert!(matches!(route.handler, RouteHandler::HealthCheck));
        assert!(params.is_empty());

        let (route, params) =
            router.match_route(&HttpMethod::Get, "/api/v1/sandboxes/test-id").unwrap();
        assert!(matches!(route.handler, RouteHandler::GetSandbox));
        assert_eq!(params.get("id"), Some(&"test-id".to_string()));
    }

    #[test]
    fn test_rate_limiter() {
        let config = RateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(60),
            include_headers: true,
        };
        let limiter = RateLimiter::new(config);

        assert!(limiter.check("client-1").is_allowed());
        assert!(limiter.check("client-1").is_allowed());
        assert!(limiter.check("client-1").is_allowed());
        assert!(!limiter.check("client-1").is_allowed());

        // Different client is not affected
        assert!(limiter.check("client-2").is_allowed());
    }

    #[test]
    fn test_authentication() {
        let config = GatewayConfig {
            api_keys: HashMap::from([
                ("key-123".to_string(), "admin".to_string()),
                ("key-456".to_string(), "user".to_string()),
            ]),
            ..Default::default()
        };
        let router = GatewayRouter::new(config);

        assert_eq!(router.authenticate("key-123"), Some("admin".to_string()));
        assert_eq!(router.authenticate("key-456"), Some("user".to_string()));
        assert_eq!(router.authenticate("invalid"), None);
    }

    #[test]
    fn test_openapi_spec() {
        let router = GatewayRouter::new(GatewayConfig::default());
        let spec = router.openapi_spec();

        assert_eq!(spec["openapi"], "3.0.3");
        assert!(spec["paths"].is_object());
        assert!(spec["paths"]["/api/v1/health"]["get"].is_object());
    }

    #[test]
    fn test_custom_route() {
        let mut router = GatewayRouter::new(GatewayConfig::default());
        router.add_route(Route::new(
            HttpMethod::Get,
            "/api/v1/custom",
            RouteHandler::Custom("my-handler".into()),
        ));

        let (route, _) = router.match_route(&HttpMethod::Get, "/api/v1/custom").unwrap();
        assert!(matches!(&route.handler, RouteHandler::Custom(s) if s == "my-handler"));
    }
}
