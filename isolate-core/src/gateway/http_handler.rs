//! HTTP-to-sandbox function router.
//!
//! Maps HTTP paths to WASM modules and routes requests directly to
//! sandbox execution, transforming HTTP request bodies into sandbox input
//! and sandbox output into HTTP responses.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::gateway::http_handler::*;
//!
//! let mut router = FunctionRouter::new(FunctionRouterConfig::default());
//! router.add_route(FunctionRoute {
//!     path: "/api/hello".to_string(),
//!     methods: vec!["GET".to_string(), "POST".to_string()],
//!     module_hash: "sha256:abc123".to_string(),
//!     timeout_ms: 30_000,
//!     max_body_size: 1024 * 1024,
//!     rate_limit_rps: Some(100),
//!     require_auth: false,
//! });
//!
//! let matched = router.match_route("/api/hello", "POST");
//! assert!(matched.is_some());
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Configuration for the function router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRouterConfig {
    /// Global rate limit (requests per second).
    pub global_rps: u32,
    /// Default timeout for function execution in ms.
    pub default_timeout_ms: u64,
    /// Maximum request body size in bytes.
    pub max_body_size: usize,
    /// Whether to enable CORS headers.
    pub enable_cors: bool,
    /// Allowed CORS origins.
    pub cors_origins: Vec<String>,
}

impl Default for FunctionRouterConfig {
    fn default() -> Self {
        Self {
            global_rps: 10_000,
            default_timeout_ms: 30_000,
            max_body_size: 10 * 1024 * 1024,
            enable_cors: true,
            cors_origins: vec![],
        }
    }
}

/// A route mapping an HTTP path to a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRoute {
    /// URL path (e.g. "/api/hello").
    pub path: String,
    /// Allowed HTTP methods (e.g. ["GET", "POST"]).
    pub methods: Vec<String>,
    /// Module hash to execute.
    pub module_hash: String,
    /// Execution timeout in ms.
    pub timeout_ms: u64,
    /// Maximum request body size in bytes.
    pub max_body_size: usize,
    /// Per-route rate limit (requests per second).
    pub rate_limit_rps: Option<u32>,
    /// Whether authentication is required.
    pub require_auth: bool,
}

/// Result of matching a route.
#[derive(Debug, Clone)]
pub struct RouteMatch {
    /// The matched route.
    pub route: FunctionRoute,
    /// Path parameters extracted from the URL.
    pub params: HashMap<String, String>,
}

/// Per-route rate tracking.
struct RouteRateState {
    counter: AtomicU64,
    window_start: std::sync::Mutex<std::time::Instant>,
}

/// An HTTP function router that maps HTTP requests to WASM modules.
pub struct FunctionRouter {
    routes: Vec<FunctionRoute>,
    config: FunctionRouterConfig,
    rate_states: HashMap<String, Arc<RouteRateState>>,
    global_counter: AtomicU64,
    global_window: std::sync::Mutex<std::time::Instant>,
}

impl FunctionRouter {
    /// Create a new function router.
    pub fn new(config: FunctionRouterConfig) -> Self {
        Self {
            routes: Vec::new(),
            config,
            rate_states: HashMap::new(),
            global_counter: AtomicU64::new(0),
            global_window: std::sync::Mutex::new(std::time::Instant::now()),
        }
    }

    /// Add a route.
    pub fn add_route(&mut self, route: FunctionRoute) {
        let path = route.path.clone();
        if route.rate_limit_rps.is_some() {
            self.rate_states.insert(
                path,
                Arc::new(RouteRateState {
                    counter: AtomicU64::new(0),
                    window_start: std::sync::Mutex::new(std::time::Instant::now()),
                }),
            );
        }
        self.routes.push(route);
    }

    /// Match an incoming request to a route.
    pub fn match_route(&self, path: &str, method: &str) -> Option<RouteMatch> {
        for route in &self.routes {
            if !route.methods.iter().any(|m| m.eq_ignore_ascii_case(method)) {
                continue;
            }

            if let Some(params) = match_path_pattern(&route.path, path) {
                return Some(RouteMatch {
                    route: route.clone(),
                    params,
                });
            }
        }
        None
    }

    /// Check if the request passes rate limiting.
    pub fn check_rate_limit(&self, path: &str) -> bool {
        // Global rate limit — check and increment inside the lock to prevent TOCTOU
        let now = std::time::Instant::now();
        {
            let mut window = self.global_window.lock().expect("global rate limit window lock poisoned");
            if now.duration_since(*window).as_secs() >= 1 {
                *window = now;
                self.global_counter.store(0, Ordering::Release);
            }
            let count = self.global_counter.fetch_add(1, Ordering::AcqRel);
            if count >= self.config.global_rps as u64 {
                return false;
            }
        }

        // Per-route rate limit
        if let Some(state) = self.rate_states.get(path) {
            let route = self.routes.iter().find(|r| r.path == path);
            if let Some(route) = route {
                if let Some(limit) = route.rate_limit_rps {
                    let mut window = state.window_start.lock().expect("route rate limit window lock poisoned");
                    if now.duration_since(*window).as_secs() >= 1 {
                        *window = now;
                        state.counter.store(0, Ordering::Release);
                    }
                    let count = state.counter.fetch_add(1, Ordering::AcqRel);
                    if count >= limit as u64 {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Get the number of registered routes.
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// List all routes.
    pub fn routes(&self) -> &[FunctionRoute] {
        &self.routes
    }

    /// Get the configuration.
    pub fn config(&self) -> &FunctionRouterConfig {
        &self.config
    }
}

/// Match a path pattern against an actual path, extracting parameters.
///
/// Pattern segments starting with `:` are treated as named parameters.
/// e.g. `/api/users/:id` matches `/api/users/123` with params `{id: "123"}`
fn match_path_pattern(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_parts: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pat, actual) in pattern_parts.iter().zip(path_parts.iter()) {
        if let Some(name) = pat.strip_prefix(':') {
            params.insert(name.to_string(), actual.to_string());
        } else if pat != actual {
            return None;
        }
    }

    Some(params)
}

/// HTTP response that the gateway would send back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl FunctionResponse {
    /// Create a 200 OK response.
    pub fn ok(body: Vec<u8>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/octet-stream".to_string());
        Self {
            status: 200,
            headers,
            body,
        }
    }

    /// Create a JSON 200 OK response.
    pub fn json(body: &impl Serialize) -> Self {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        Self {
            status: 200,
            headers,
            body: serde_json::to_vec(body).unwrap_or_default(),
        }
    }

    /// Create an error response.
    pub fn error(status: u16, message: &str) -> Self {
        let body = serde_json::json!({"error": message});
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        Self {
            status,
            headers,
            body: serde_json::to_vec(&body).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_route() -> FunctionRoute {
        FunctionRoute {
            path: "/api/hello".to_string(),
            methods: vec!["GET".to_string(), "POST".to_string()],
            module_hash: "sha256:abc".to_string(),
            timeout_ms: 30_000,
            max_body_size: 1024,
            rate_limit_rps: Some(100),
            require_auth: false,
        }
    }

    #[test]
    fn test_add_and_match_route() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(test_route());

        let m = router.match_route("/api/hello", "GET");
        assert!(m.is_some());
        assert!(m.unwrap().params.is_empty());
    }

    #[test]
    fn test_no_match_wrong_method() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(test_route());

        assert!(router.match_route("/api/hello", "DELETE").is_none());
    }

    #[test]
    fn test_no_match_wrong_path() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(test_route());

        assert!(router.match_route("/api/other", "GET").is_none());
    }

    #[test]
    fn test_path_params() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(FunctionRoute {
            path: "/api/users/:id/actions/:action".to_string(),
            methods: vec!["POST".to_string()],
            module_hash: "sha256:def".to_string(),
            timeout_ms: 5000,
            max_body_size: 1024,
            rate_limit_rps: None,
            require_auth: false,
        });

        let m = router.match_route("/api/users/42/actions/run", "POST").unwrap();
        assert_eq!(m.params.get("id").unwrap(), "42");
        assert_eq!(m.params.get("action").unwrap(), "run");
    }

    #[test]
    fn test_rate_limiting() {
        let mut router = FunctionRouter::new(FunctionRouterConfig {
            global_rps: 3,
            ..FunctionRouterConfig::default()
        });
        router.add_route(test_route());

        assert!(router.check_rate_limit("/api/hello"));
        assert!(router.check_rate_limit("/api/hello"));
        assert!(router.check_rate_limit("/api/hello"));
        // 4th request in same second should be rate-limited
        assert!(!router.check_rate_limit("/api/hello"));
    }

    #[test]
    fn test_function_response_ok() {
        let resp = FunctionResponse::ok(b"hello".to_vec());
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn test_function_response_error() {
        let resp = FunctionResponse::error(429, "rate limited");
        assert_eq!(resp.status, 429);
        let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(body["error"], "rate limited");
    }

    #[test]
    fn test_route_count() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        assert_eq!(router.route_count(), 0);
        router.add_route(test_route());
        assert_eq!(router.route_count(), 1);
    }

    #[test]
    fn test_match_path_pattern() {
        assert!(match_path_pattern("/a/b", "/a/b").is_some());
        assert!(match_path_pattern("/a/b", "/a/c").is_none());
        assert!(match_path_pattern("/a/:x", "/a/123").is_some());
        assert!(match_path_pattern("/a/b/c", "/a/b").is_none());
    }

    #[test]
    fn test_exact_path_matching() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(FunctionRoute {
            path: "/exact/path".to_string(),
            methods: vec!["GET".to_string()],
            module_hash: "hash".to_string(),
            timeout_ms: 1000,
            max_body_size: 1024,
            rate_limit_rps: None,
            require_auth: false,
        });

        assert!(router.match_route("/exact/path", "GET").is_some());
        assert!(router.match_route("/exact/path/extra", "GET").is_none());
        assert!(router.match_route("/exact", "GET").is_none());
    }

    #[test]
    fn test_parameterized_path_matching() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(FunctionRoute {
            path: "/users/:id/posts/:post_id".to_string(),
            methods: vec!["GET".to_string()],
            module_hash: "hash".to_string(),
            timeout_ms: 1000,
            max_body_size: 1024,
            rate_limit_rps: None,
            require_auth: false,
        });

        let m = router.match_route("/users/42/posts/99", "GET").unwrap();
        assert_eq!(m.params.get("id").unwrap(), "42");
        assert_eq!(m.params.get("post_id").unwrap(), "99");
    }

    #[test]
    fn test_url_encoded_parameters() {
        // URL-encoded params pass through as-is (no decoding in router)
        let params = match_path_pattern("/api/:path", "/api/%2e%2e").unwrap();
        assert_eq!(params.get("path").unwrap(), "%2e%2e");
    }

    #[test]
    fn test_path_traversal_in_params() {
        // ../.. in path params are matched literally, not traversed
        let params = match_path_pattern("/files/:name", "/files/..%2F..%2Fetc%2Fpasswd").unwrap();
        assert_eq!(params.get("name").unwrap(), "..%2F..%2Fetc%2Fpasswd");
    }

    #[test]
    fn test_per_route_rate_limit() {
        let mut router = FunctionRouter::new(FunctionRouterConfig {
            global_rps: 10_000,
            ..FunctionRouterConfig::default()
        });
        router.add_route(FunctionRoute {
            path: "/limited".to_string(),
            methods: vec!["GET".to_string()],
            module_hash: "hash".to_string(),
            timeout_ms: 1000,
            max_body_size: 1024,
            rate_limit_rps: Some(2),
            require_auth: false,
        });

        assert!(router.check_rate_limit("/limited"));
        assert!(router.check_rate_limit("/limited"));
        assert!(!router.check_rate_limit("/limited"));
    }

    #[test]
    fn test_case_insensitive_method_matching() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(test_route());

        assert!(router.match_route("/api/hello", "get").is_some());
        assert!(router.match_route("/api/hello", "Get").is_some());
        assert!(router.match_route("/api/hello", "GET").is_some());
    }

    #[test]
    fn test_empty_routes() {
        let router = FunctionRouter::new(FunctionRouterConfig::default());
        assert_eq!(router.route_count(), 0);
        assert!(router.match_route("/anything", "GET").is_none());
    }

    #[test]
    fn test_function_response_json() {
        let data = serde_json::json!({"key": "value"});
        let resp = FunctionResponse::json(&data);
        assert_eq!(resp.status, 200);
        assert_eq!(
            resp.headers.get("content-type").unwrap(),
            "application/json"
        );
        let parsed: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_multiple_routes_first_match_wins() {
        let mut router = FunctionRouter::new(FunctionRouterConfig::default());
        router.add_route(FunctionRoute {
            path: "/api/:id".to_string(),
            methods: vec!["GET".to_string()],
            module_hash: "first".to_string(),
            timeout_ms: 1000,
            max_body_size: 1024,
            rate_limit_rps: None,
            require_auth: false,
        });
        router.add_route(FunctionRoute {
            path: "/api/:name".to_string(),
            methods: vec!["GET".to_string()],
            module_hash: "second".to_string(),
            timeout_ms: 1000,
            max_body_size: 1024,
            rate_limit_rps: None,
            require_auth: false,
        });

        let m = router.match_route("/api/test", "GET").unwrap();
        assert_eq!(m.route.module_hash, "first");
    }

    #[test]
    fn test_trailing_slash_handling() {
        // match_path_pattern trims slashes, so /a/b/ == /a/b
        assert!(match_path_pattern("/a/b", "/a/b/").is_some());
        assert!(match_path_pattern("/a/b/", "/a/b").is_some());
    }

    #[test]
    fn test_config_defaults() {
        let config = FunctionRouterConfig::default();
        assert_eq!(config.global_rps, 10_000);
        assert_eq!(config.default_timeout_ms, 30_000);
        assert!(config.enable_cors);
    }
}
