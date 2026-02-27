//! Pre-defined spans for sandbox operations.

// This module is experimental and not all APIs are used yet.
#![allow(dead_code)]

use opentelemetry::{
    global,
    trace::{Span, SpanKind, Status, Tracer},
    KeyValue,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Span attribute keys for sandbox operations.
pub mod attributes {
    /// Sandbox ID attribute.
    pub const SANDBOX_ID: &str = "sandbox.id";
    /// Module hash attribute.
    pub const MODULE_HASH: &str = "sandbox.module.hash";
    /// Exit code attribute.
    pub const EXIT_CODE: &str = "sandbox.exit_code";
    /// Memory usage attribute.
    pub const MEMORY_BYTES: &str = "sandbox.memory.bytes";
    /// Fuel consumed attribute.
    pub const FUEL_CONSUMED: &str = "sandbox.fuel.consumed";
    /// Execution duration attribute.
    pub const DURATION_MS: &str = "sandbox.duration.ms";
    /// Capability name attribute.
    pub const CAPABILITY: &str = "sandbox.capability";
    /// Operation name attribute.
    pub const OPERATION: &str = "sandbox.operation";
    /// Error type attribute.
    pub const ERROR_TYPE: &str = "error.type";
    /// Error message attribute.
    pub const ERROR_MESSAGE: &str = "error.message";
    /// Snapshot ID attribute.
    pub const SNAPSHOT_ID: &str = "sandbox.snapshot.id";
    /// HTTP method attribute.
    pub const HTTP_METHOD: &str = "http.method";
    /// HTTP URL attribute.
    pub const HTTP_URL: &str = "http.url";
    /// HTTP status code attribute.
    pub const HTTP_STATUS_CODE: &str = "http.status_code";
}

/// A wrapper around OpenTelemetry span for sandbox operations.
pub struct SandboxSpan {
    span: opentelemetry::global::BoxedSpan,
    start_time: Instant,
}

impl SandboxSpan {
    /// Start a new span.
    fn new(name: &'static str, kind: SpanKind, attributes: Vec<KeyValue>) -> Self {
        let tracer = global::tracer("isolate");
        let span =
            tracer.span_builder(name).with_kind(kind).with_attributes(attributes).start(&tracer);

        Self { span, start_time: Instant::now() }
    }

    /// Add an attribute to the span.
    pub fn set_attribute(&mut self, key: &str, value: impl Into<AttributeValue>) {
        let value = value.into();
        let kv = match value {
            AttributeValue::String(s) => KeyValue::new(key.to_string(), s),
            AttributeValue::Int(i) => KeyValue::new(key.to_string(), i),
            AttributeValue::Float(f) => KeyValue::new(key.to_string(), f),
            AttributeValue::Bool(b) => KeyValue::new(key.to_string(), b),
        };
        self.span.set_attribute(kv);
    }

    /// Record an error on the span.
    pub fn record_error(&mut self, error: &str) {
        self.span.set_attribute(KeyValue::new(attributes::ERROR_MESSAGE, error.to_string()));
        self.span.set_status(Status::error(error.to_string()));
    }

    /// Mark the span as successful.
    pub fn set_ok(&mut self) {
        self.span.set_status(Status::Ok);
    }

    /// Get the elapsed time since span start.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// End the span.
    pub fn end(mut self) {
        let duration_ms = self.start_time.elapsed().as_millis() as i64;
        self.span.set_attribute(KeyValue::new(attributes::DURATION_MS, duration_ms));
        self.span.end();
    }

    /// End the span with success status.
    pub fn end_ok(mut self) {
        self.set_ok();
        self.end();
    }

    /// End the span with error status.
    pub fn end_error(mut self, error: &str) {
        self.record_error(error);
        self.end();
    }
}

/// Attribute value types.
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl From<String> for AttributeValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for AttributeValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for AttributeValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<i32> for AttributeValue {
    fn from(i: i32) -> Self {
        Self::Int(i as i64)
    }
}

impl From<u64> for AttributeValue {
    fn from(i: u64) -> Self {
        Self::Int(i as i64)
    }
}

impl From<usize> for AttributeValue {
    fn from(i: usize) -> Self {
        Self::Int(i as i64)
    }
}

impl From<f64> for AttributeValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for AttributeValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Builder for creating sandbox spans.
pub struct SpanBuilder {
    name: &'static str,
    kind: SpanKind,
    attributes: Vec<KeyValue>,
}

impl SpanBuilder {
    /// Create a new span builder.
    pub fn new(name: &'static str) -> Self {
        Self { name, kind: SpanKind::Internal, attributes: Vec::new() }
    }

    /// Set the span kind.
    pub fn kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add a sandbox ID attribute.
    pub fn sandbox_id(mut self, id: Uuid) -> Self {
        self.attributes.push(KeyValue::new(attributes::SANDBOX_ID, id.to_string()));
        self
    }

    /// Add a module hash attribute.
    pub fn module_hash(mut self, hash: &str) -> Self {
        self.attributes.push(KeyValue::new(attributes::MODULE_HASH, hash.to_string()));
        self
    }

    /// Add a string attribute.
    pub fn attribute(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attributes.push(KeyValue::new(key.to_string(), value.into()));
        self
    }

    /// Add an integer attribute.
    pub fn attribute_i64(mut self, key: &str, value: i64) -> Self {
        self.attributes.push(KeyValue::new(key.to_string(), value));
        self
    }

    /// Start the span.
    pub fn start(self) -> SandboxSpan {
        SandboxSpan::new(self.name, self.kind, self.attributes)
    }
}

/// Create a span for sandbox creation.
pub fn sandbox_create(sandbox_id: Uuid, module_hash: Option<&str>) -> SandboxSpan {
    let mut builder =
        SpanBuilder::new("sandbox.create").kind(SpanKind::Internal).sandbox_id(sandbox_id);

    if let Some(hash) = module_hash {
        builder = builder.module_hash(hash);
    }

    builder.start()
}

/// Create a span for sandbox execution.
pub fn sandbox_execute(sandbox_id: Uuid) -> SandboxSpan {
    SpanBuilder::new("sandbox.execute").kind(SpanKind::Internal).sandbox_id(sandbox_id).start()
}

/// Create a span for sandbox termination.
pub fn sandbox_terminate(sandbox_id: Uuid, reason: &str) -> SandboxSpan {
    SpanBuilder::new("sandbox.terminate")
        .kind(SpanKind::Internal)
        .sandbox_id(sandbox_id)
        .attribute("terminate.reason", reason)
        .start()
}

/// Create a span for WASM module compilation.
pub fn module_compile(module_hash: &str) -> SandboxSpan {
    SpanBuilder::new("module.compile").kind(SpanKind::Internal).module_hash(module_hash).start()
}

/// Create a span for snapshot creation.
pub fn snapshot_create(sandbox_id: Uuid, snapshot_id: &str) -> SandboxSpan {
    SpanBuilder::new("snapshot.create")
        .kind(SpanKind::Internal)
        .sandbox_id(sandbox_id)
        .attribute(attributes::SNAPSHOT_ID, snapshot_id)
        .start()
}

/// Create a span for snapshot restore.
pub fn snapshot_restore(sandbox_id: Uuid, snapshot_id: &str) -> SandboxSpan {
    SpanBuilder::new("snapshot.restore")
        .kind(SpanKind::Internal)
        .sandbox_id(sandbox_id)
        .attribute(attributes::SNAPSHOT_ID, snapshot_id)
        .start()
}

/// Create a span for capability check.
pub fn capability_check(sandbox_id: Uuid, capability: &str) -> SandboxSpan {
    SpanBuilder::new("capability.check")
        .kind(SpanKind::Internal)
        .sandbox_id(sandbox_id)
        .attribute(attributes::CAPABILITY, capability)
        .start()
}

/// Create a span for HTTP request from sandbox.
pub fn http_request(sandbox_id: Uuid, method: &str, url: &str) -> SandboxSpan {
    SpanBuilder::new("http.request")
        .kind(SpanKind::Client)
        .sandbox_id(sandbox_id)
        .attribute(attributes::HTTP_METHOD, method)
        .attribute(attributes::HTTP_URL, url)
        .start()
}

/// Create a span for audit log entry.
pub fn audit_log(sandbox_id: Uuid, action: &str) -> SandboxSpan {
    SpanBuilder::new("audit.log")
        .kind(SpanKind::Internal)
        .sandbox_id(sandbox_id)
        .attribute(attributes::OPERATION, action)
        .start()
}

/// Create a span for signature verification.
pub fn signature_verify(module_hash: &str) -> SandboxSpan {
    SpanBuilder::new("signature.verify").kind(SpanKind::Internal).module_hash(module_hash).start()
}

/// Convenience macro for creating spans with automatic error handling.
#[macro_export]
macro_rules! with_span {
    ($span:expr, $body:expr) => {{
        let mut span = $span;
        let result = $body;
        match &result {
            Ok(_) => span.end_ok(),
            Err(e) => span.end_error(&e.to_string()),
        }
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_builder() {
        let sandbox_id = Uuid::new_v4();
        let span = SpanBuilder::new("test.span")
            .kind(SpanKind::Internal)
            .sandbox_id(sandbox_id)
            .attribute("custom", "value")
            .attribute_i64("count", 42)
            .start();

        assert!(span.elapsed() >= Duration::ZERO);
        span.end();
    }

    #[test]
    fn test_span_attribute_values() {
        let mut span = SpanBuilder::new("test.attributes").start();

        span.set_attribute("string", "value");
        span.set_attribute("int", 42i64);
        span.set_attribute("float", 2.5f64);
        span.set_attribute("bool", true);

        span.end_ok();
    }

    #[test]
    fn test_span_error() {
        let mut span = SpanBuilder::new("test.error").start();

        span.record_error("Test error message");
        span.end();
    }

    #[test]
    fn test_predefined_spans() {
        let sandbox_id = Uuid::new_v4();

        // Test all predefined spans
        sandbox_create(sandbox_id, Some("abc123")).end();
        sandbox_execute(sandbox_id).end_ok();
        sandbox_terminate(sandbox_id, "user request").end();
        module_compile("hash123").end_ok();
        snapshot_create(sandbox_id, "snap-1").end();
        snapshot_restore(sandbox_id, "snap-1").end();
        capability_check(sandbox_id, "stdout").end_ok();
        http_request(sandbox_id, "GET", "https://example.com").end();
        audit_log(sandbox_id, "sandbox_created").end();
        signature_verify("hash123").end_ok();
    }

    #[test]
    fn test_attribute_conversions() {
        let _: AttributeValue = "string".into();
        let _: AttributeValue = String::from("owned").into();
        let _: AttributeValue = 42i64.into();
        let _: AttributeValue = 42i32.into();
        let _: AttributeValue = 42u64.into();
        let _: AttributeValue = 42usize.into();
        let _: AttributeValue = 2.5f64.into();
        let _: AttributeValue = true.into();
    }
}
