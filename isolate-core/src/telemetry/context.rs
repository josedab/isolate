//! Trace context propagation utilities.

use opentelemetry::{
    global,
    propagation::{Extractor, Injector},
    Context,
};
use std::collections::HashMap;

/// Trace context for propagation across service boundaries.
#[derive(Debug, Clone, Default)]
pub struct TraceContext {
    /// Headers for propagation.
    headers: HashMap<String, String>,
}

impl TraceContext {
    /// Create an empty trace context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a trace context from headers.
    pub fn from_headers(headers: HashMap<String, String>) -> Self {
        Self { headers }
    }

    /// Get the headers for propagation.
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Get a specific header value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(|s| s.as_str())
    }

    /// Set a header value.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.headers.insert(key.into(), value.into());
    }

    /// Check if the context is empty.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// Get trace ID if available.
    pub fn trace_id(&self) -> Option<&str> {
        self.headers.get("traceparent").map(|s| {
            // traceparent format: version-trace_id-span_id-flags
            s.split('-').nth(1).unwrap_or(s.as_str())
        })
    }

    /// Merge another context into this one.
    pub fn merge(&mut self, other: &TraceContext) {
        for (key, value) in &other.headers {
            self.headers.insert(key.clone(), value.clone());
        }
    }
}

impl Extractor for TraceContext {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(|s| s.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(|s| s.as_str()).collect()
    }
}

impl Injector for TraceContext {
    fn set(&mut self, key: &str, value: String) {
        self.headers.insert(key.to_string(), value);
    }
}

/// Extract trace context from a carrier.
pub fn extract_context(carrier: &TraceContext) -> Context {
    let propagator = global::get_text_map_propagator(|propagator| propagator.extract(carrier));
    propagator
}

/// Inject current trace context into a carrier.
pub fn inject_context(context: &Context, carrier: &mut TraceContext) {
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(context, carrier);
    });
}

/// Extract trace context from HTTP headers.
pub fn extract_from_http_headers(headers: &[(String, String)]) -> TraceContext {
    let mut ctx = TraceContext::new();
    for (key, value) in headers {
        ctx.set(key.to_lowercase(), value);
    }
    ctx
}

/// Create HTTP headers from trace context.
pub fn create_http_headers(context: &TraceContext) -> Vec<(String, String)> {
    context
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// W3C TraceContext format constants.
pub mod w3c {
    /// W3C traceparent header name.
    pub const TRACEPARENT: &str = "traceparent";
    /// W3C tracestate header name.
    pub const TRACESTATE: &str = "tracestate";

    /// Parse a traceparent header.
    pub fn parse_traceparent(header: &str) -> Option<TraceparentData> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        Some(TraceparentData {
            version: parts[0].to_string(),
            trace_id: parts[1].to_string(),
            parent_id: parts[2].to_string(),
            flags: parts[3].to_string(),
        })
    }

    /// Create a traceparent header.
    pub fn create_traceparent(
        version: &str,
        trace_id: &str,
        span_id: &str,
        sampled: bool,
    ) -> String {
        let flags = if sampled { "01" } else { "00" };
        format!("{}-{}-{}-{}", version, trace_id, span_id, flags)
    }

    /// Parsed traceparent data.
    #[derive(Debug, Clone)]
    pub struct TraceparentData {
        /// Version (should be "00").
        pub version: String,
        /// Trace ID (32 hex characters).
        pub trace_id: String,
        /// Parent span ID (16 hex characters).
        pub parent_id: String,
        /// Flags (2 hex characters).
        pub flags: String,
    }

    impl TraceparentData {
        /// Check if the trace is sampled.
        pub fn is_sampled(&self) -> bool {
            self.flags.ends_with('1')
        }
    }
}

/// Baggage context for passing arbitrary key-value pairs.
#[derive(Debug, Clone, Default)]
pub struct BaggageContext {
    items: HashMap<String, String>,
}

impl BaggageContext {
    /// Create an empty baggage context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a baggage item.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.items.insert(key.into(), value.into());
    }

    /// Get a baggage item.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.get(key).map(|s| s.as_str())
    }

    /// Remove a baggage item.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.items.remove(key)
    }

    /// Check if baggage is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get all items.
    pub fn items(&self) -> &HashMap<String, String> {
        &self.items
    }

    /// Encode to baggage header format.
    pub fn to_header(&self) -> String {
        self.items
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse from baggage header format.
    pub fn from_header(header: &str) -> Self {
        let mut ctx = Self::new();
        for item in header.split(',') {
            if let Some((key, value)) = item.split_once('=') {
                ctx.set(key.trim(), value.trim());
            }
        }
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_creation() {
        let ctx = TraceContext::new();
        assert!(ctx.is_empty());
    }

    #[test]
    fn test_trace_context_headers() {
        let mut ctx = TraceContext::new();
        ctx.set("traceparent", "00-abc123-def456-01");
        ctx.set("custom-header", "value");

        assert_eq!(ctx.get("traceparent"), Some("00-abc123-def456-01"));
        assert_eq!(ctx.get("custom-header"), Some("value"));
        assert_eq!(ctx.get("missing"), None);
    }

    #[test]
    fn test_trace_context_merge() {
        let mut ctx1 = TraceContext::new();
        ctx1.set("key1", "value1");

        let mut ctx2 = TraceContext::new();
        ctx2.set("key2", "value2");

        ctx1.merge(&ctx2);
        assert_eq!(ctx1.get("key1"), Some("value1"));
        assert_eq!(ctx1.get("key2"), Some("value2"));
    }

    #[test]
    fn test_trace_id_extraction() {
        let mut ctx = TraceContext::new();
        ctx.set(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        );

        assert_eq!(ctx.trace_id(), Some("0af7651916cd43dd8448eb211c80319c"));
    }

    #[test]
    fn test_w3c_traceparent_parsing() {
        let header = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let data = w3c::parse_traceparent(header).unwrap();

        assert_eq!(data.version, "00");
        assert_eq!(data.trace_id, "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(data.parent_id, "b7ad6b7169203331");
        assert_eq!(data.flags, "01");
        assert!(data.is_sampled());
    }

    #[test]
    fn test_w3c_traceparent_creation() {
        let header = w3c::create_traceparent(
            "00",
            "0af7651916cd43dd8448eb211c80319c",
            "b7ad6b7169203331",
            true,
        );

        assert_eq!(
            header,
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
        );
    }

    #[test]
    fn test_w3c_traceparent_invalid() {
        assert!(w3c::parse_traceparent("invalid").is_none());
        assert!(w3c::parse_traceparent("00-abc").is_none());
    }

    #[test]
    fn test_baggage_context() {
        let mut baggage = BaggageContext::new();
        assert!(baggage.is_empty());

        baggage.set("user_id", "12345");
        baggage.set("tenant", "acme");

        assert_eq!(baggage.get("user_id"), Some("12345"));
        assert_eq!(baggage.get("tenant"), Some("acme"));
        assert!(!baggage.is_empty());

        let removed = baggage.remove("user_id");
        assert_eq!(removed, Some("12345".to_string()));
        assert_eq!(baggage.get("user_id"), None);
    }

    #[test]
    fn test_baggage_header_roundtrip() {
        let mut baggage = BaggageContext::new();
        baggage.set("key1", "value1");
        baggage.set("key2", "value2");

        let header = baggage.to_header();
        let parsed = BaggageContext::from_header(&header);

        assert_eq!(parsed.get("key1"), Some("value1"));
        assert_eq!(parsed.get("key2"), Some("value2"));
    }

    #[test]
    fn test_extract_from_http_headers() {
        let headers = vec![
            ("Traceparent".to_string(), "00-trace-span-01".to_string()),
            ("X-Custom".to_string(), "value".to_string()),
        ];

        let ctx = extract_from_http_headers(&headers);
        assert_eq!(ctx.get("traceparent"), Some("00-trace-span-01"));
        assert_eq!(ctx.get("x-custom"), Some("value"));
    }

    #[test]
    fn test_create_http_headers() {
        let mut ctx = TraceContext::new();
        ctx.set("traceparent", "00-abc-def-01");
        ctx.set("tracestate", "vendor=value");

        let headers = create_http_headers(&ctx);
        assert_eq!(headers.len(), 2);
    }
}
