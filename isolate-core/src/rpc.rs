//! Inter-sandbox RPC (Remote Procedure Call) framework.
//!
//! Enables sandboxes to communicate with each other through a capability-gated
//! RPC mechanism with automatic trace propagation and circuit breaker patterns.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::rpc::{RpcRegistry, RpcRequest, RpcResponse};
//! use isolate_core::sandbox::SandboxId;
//!
//! let registry = RpcRegistry::new();
//!
//! // Register a sandbox as an RPC endpoint
//! let sandbox_id = SandboxId::new();
//! registry.register(sandbox_id, "calculator".to_string());
//!
//! // Create an RPC request
//! let request = RpcRequest::new("calculator", "add")
//!     .with_payload(b"{\"a\": 1, \"b\": 2}".to_vec());
//! ```

use crate::sandbox::SandboxId;

use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Registry mapping service names to sandbox endpoints.
pub struct RpcRegistry {
    services: DashMap<String, SandboxId>,
    stats: Arc<RpcStats>,
}

impl RpcRegistry {
    /// Create a new RPC registry.
    pub fn new() -> Self {
        Self {
            services: DashMap::new(),
            stats: Arc::new(RpcStats::default()),
        }
    }

    /// Register a sandbox as a named service endpoint.
    pub fn register(&self, sandbox_id: SandboxId, service_name: String) {
        self.services.insert(service_name, sandbox_id);
    }

    /// Unregister a service.
    pub fn unregister(&self, service_name: &str) {
        self.services.remove(service_name);
    }

    /// Resolve a service name to a sandbox ID.
    pub fn resolve(&self, service_name: &str) -> Option<SandboxId> {
        self.services.get(service_name).map(|v| *v)
    }

    /// List all registered services.
    pub fn services(&self) -> Vec<(String, SandboxId)> {
        self.services.iter().map(|e| (e.key().clone(), *e.value())).collect()
    }

    /// Get RPC statistics.
    pub fn stats(&self) -> &RpcStats {
        &self.stats
    }
}

impl Default for RpcRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// An RPC request from one sandbox to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Target service name.
    pub service: String,
    /// Method to invoke.
    pub method: String,
    /// Request payload (typically JSON or MessagePack).
    pub payload: Vec<u8>,
    /// Trace context for distributed tracing propagation.
    pub trace_context: Option<TraceContext>,
    /// Request timeout.
    pub timeout: Option<Duration>,
}

impl RpcRequest {
    /// Create a new RPC request.
    pub fn new(service: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            method: method.into(),
            payload: Vec::new(),
            trace_context: None,
            timeout: None,
        }
    }

    /// Set the request payload.
    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }

    /// Set trace context for propagation.
    pub fn with_trace_context(mut self, ctx: TraceContext) -> Self {
        self.trace_context = Some(ctx);
        self
    }

    /// Set request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// An RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Whether the call succeeded.
    pub success: bool,
    /// Response payload.
    pub payload: Vec<u8>,
    /// Error message, if any.
    pub error: Option<String>,
    /// Execution duration on the target sandbox.
    pub duration: Duration,
}

impl RpcResponse {
    /// Create a successful response.
    pub fn ok(payload: Vec<u8>, duration: Duration) -> Self {
        Self { success: true, payload, error: None, duration }
    }

    /// Create an error response.
    pub fn error(message: impl Into<String>, duration: Duration) -> Self {
        Self {
            success: false,
            payload: Vec::new(),
            error: Some(message.into()),
            duration,
        }
    }
}

/// Distributed tracing context for cross-sandbox propagation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    /// Trace ID (128-bit hex string).
    pub trace_id: String,
    /// Parent span ID (64-bit hex string).
    pub span_id: String,
    /// Trace flags (e.g., sampled).
    pub flags: u8,
    /// Baggage items (key-value metadata).
    pub baggage: HashMap<String, String>,
}

impl TraceContext {
    /// Create a new trace context.
    pub fn new(trace_id: impl Into<String>, span_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            flags: 1, // Sampled by default
            baggage: HashMap::new(),
        }
    }

    /// Add a baggage item.
    pub fn with_baggage(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.baggage.insert(key.into(), value.into());
        self
    }

    /// Check if the trace is sampled.
    pub fn is_sampled(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Format as W3C traceparent header value.
    pub fn to_traceparent(&self) -> String {
        format!("00-{}-{}-{:02x}", self.trace_id, self.span_id, self.flags)
    }
}

/// Circuit breaker for protecting against cascade failures in RPC calls.
pub struct CircuitBreaker {
    state: Mutex<CircuitBreakerState>,
    config: CircuitBreakerConfig,
}

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit.
    pub failure_threshold: u32,
    /// Duration to stay open before moving to half-open.
    pub open_duration: Duration,
    /// Number of successes in half-open state before closing.
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Normal operation - requests pass through.
    Closed,
    /// Circuit tripped - requests are rejected immediately.
    Open,
    /// Testing recovery - limited requests allowed.
    HalfOpen,
}

struct CircuitBreakerState {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure: Option<Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Mutex::new(CircuitBreakerState {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure: None,
            }),
            config,
        }
    }

    /// Check if a request should be allowed.
    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock();
        match state.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if open duration has elapsed
                if let Some(last_failure) = state.last_failure {
                    if last_failure.elapsed() >= self.config.open_duration {
                        state.state = CircuitState::HalfOpen;
                        state.success_count = 0;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        let mut state = self.state.lock();
        match state.state {
            CircuitState::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.config.success_threshold {
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                }
            }
            CircuitState::Closed => {
                state.failure_count = 0; // Reset on success
            }
            _ => {}
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        let mut state = self.state.lock();
        state.failure_count += 1;
        state.last_failure = Some(Instant::now());

        if state.failure_count >= self.config.failure_threshold {
            state.state = CircuitState::Open;
        }
    }

    /// Get the current circuit state.
    pub fn state(&self) -> CircuitState {
        self.state.lock().state
    }
}

/// RPC call statistics.
#[derive(Debug, Default)]
pub struct RpcStats {
    pub calls_total: AtomicU64,
    pub calls_success: AtomicU64,
    pub calls_failed: AtomicU64,
    pub calls_circuit_broken: AtomicU64,
}

impl RpcStats {
    /// Record a completed call.
    pub fn record_call(&self, success: bool) {
        self.calls_total.fetch_add(1, Ordering::Relaxed);
        if success {
            self.calls_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.calls_failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a circuit-broken call (not attempted).
    pub fn record_circuit_broken(&self) {
        self.calls_total.fetch_add(1, Ordering::Relaxed);
        self.calls_circuit_broken.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the success rate (0.0 - 1.0).
    pub fn success_rate(&self) -> f64 {
        let total = self.calls_total.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        self.calls_success.load(Ordering::Relaxed) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_registry() {
        let registry = RpcRegistry::new();
        let id = SandboxId::new();

        registry.register(id, "calculator".to_string());
        assert_eq!(registry.resolve("calculator"), Some(id));
        assert_eq!(registry.resolve("nonexistent"), None);
        assert_eq!(registry.services().len(), 1);

        registry.unregister("calculator");
        assert_eq!(registry.resolve("calculator"), None);
    }

    #[test]
    fn test_rpc_request_builder() {
        let req = RpcRequest::new("service", "method")
            .with_payload(b"hello".to_vec())
            .with_timeout(Duration::from_secs(5));

        assert_eq!(req.service, "service");
        assert_eq!(req.method, "method");
        assert_eq!(req.payload, b"hello");
        assert_eq!(req.timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_rpc_response() {
        let ok = RpcResponse::ok(b"result".to_vec(), Duration::from_millis(10));
        assert!(ok.success);
        assert!(ok.error.is_none());

        let err = RpcResponse::error("failed", Duration::from_millis(1));
        assert!(!err.success);
        assert_eq!(err.error, Some("failed".to_string()));
    }

    #[test]
    fn test_trace_context() {
        let ctx = TraceContext::new("abc123", "def456")
            .with_baggage("user_id", "u123");

        assert!(ctx.is_sampled());
        assert_eq!(ctx.baggage.get("user_id"), Some(&"u123".to_string()));

        let traceparent = ctx.to_traceparent();
        assert!(traceparent.starts_with("00-abc123-def456-01"));
    }

    #[test]
    fn test_circuit_breaker_closed() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });

        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_opens() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration: Duration::from_secs(60),
            ..Default::default()
        });

        // Trip the circuit
        for _ in 0..3 {
            cb.record_failure();
        }

        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_half_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            open_duration: Duration::from_millis(10),
            success_threshold: 1,
        });

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for open duration
        std::thread::sleep(Duration::from_millis(20));

        // Should transition to half-open
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // One success should close it
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_rpc_stats() {
        let stats = RpcStats::default();
        assert_eq!(stats.success_rate(), 1.0); // No calls = 100% success

        stats.record_call(true);
        stats.record_call(true);
        stats.record_call(false);
        assert!((stats.success_rate() - 0.666).abs() < 0.01);

        stats.record_circuit_broken();
        assert_eq!(stats.calls_circuit_broken.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_circuit_breaker_five_failures_opens() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);

        for _ in 0..5 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_circuit_breaker_success_resets_failure_count() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        });

        // 4 failures then 1 success should reset
        for _ in 0..4 {
            cb.record_failure();
        }
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);

        // Need 5 fresh failures to open
        for _ in 0..4 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_full_cycle() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            open_duration: Duration::from_millis(10),
            success_threshold: 1,
        });

        // Closed → Open
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Open → HalfOpen (after timeout)
        std::thread::sleep(Duration::from_millis(20));
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // HalfOpen → Closed (on success)
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            open_duration: Duration::from_millis(10),
            success_threshold: 2,
        });

        // Trip to Open
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for HalfOpen
        std::thread::sleep(Duration::from_millis(20));
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Failure in HalfOpen re-opens the circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_half_open_needs_threshold_successes() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            open_duration: Duration::from_millis(10),
            success_threshold: 3,
        });

        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        cb.allow_request(); // transition to HalfOpen

        // 2 successes are not enough
        cb.record_success();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // 3rd success closes it
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_trace_context_to_traceparent_format() {
        let ctx = TraceContext::new(
            "0af7651916cd43dd8448eb211c80319c",
            "b7ad6b7169203331",
        );
        let tp = ctx.to_traceparent();
        // W3C format: version-trace_id-span_id-flags
        assert!(tp.starts_with("00-"));
        let parts: Vec<&str> = tp.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "00"); // version
        assert_eq!(parts[1], "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(parts[2], "b7ad6b7169203331");
        assert_eq!(parts[3], "01"); // sampled flag
    }

    #[test]
    fn test_trace_context_unsampled() {
        let mut ctx = TraceContext::new("abc", "def");
        ctx.flags = 0;
        assert!(!ctx.is_sampled());
        assert!(ctx.to_traceparent().ends_with("-00"));
    }

    #[test]
    fn test_registry_add_remove_lookup() {
        let registry = RpcRegistry::new();
        let id1 = SandboxId::new();
        let id2 = SandboxId::new();

        registry.register(id1, "svc-a".into());
        registry.register(id2, "svc-b".into());
        assert_eq!(registry.services().len(), 2);

        assert_eq!(registry.resolve("svc-a"), Some(id1));
        assert_eq!(registry.resolve("svc-b"), Some(id2));

        registry.unregister("svc-a");
        assert_eq!(registry.resolve("svc-a"), None);
        assert_eq!(registry.resolve("svc-b"), Some(id2));
        assert_eq!(registry.services().len(), 1);
    }

    #[test]
    fn test_registry_overwrite() {
        let registry = RpcRegistry::new();
        let id1 = SandboxId::new();
        let id2 = SandboxId::new();

        registry.register(id1, "svc".into());
        registry.register(id2, "svc".into());
        assert_eq!(registry.resolve("svc"), Some(id2));
        assert_eq!(registry.services().len(), 1);
    }

    #[test]
    fn test_rpc_request_with_trace_context() {
        let ctx = TraceContext::new("trace1", "span1");
        let req = RpcRequest::new("svc", "method").with_trace_context(ctx);
        assert!(req.trace_context.is_some());
        let tc = req.trace_context.unwrap();
        assert_eq!(tc.trace_id, "trace1");
    }
}
