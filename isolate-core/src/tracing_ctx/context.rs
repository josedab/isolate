use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// W3C Trace Context flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceFlags(u8);

impl TraceFlags {
    pub const NONE: TraceFlags = TraceFlags(0x00);
    pub const SAMPLED: TraceFlags = TraceFlags(0x01);

    pub fn new(value: u8) -> Self {
        Self(value)
    }

    pub fn is_sampled(&self) -> bool {
        self.0 & 0x01 != 0
    }

    pub fn as_u8(&self) -> u8 {
        self.0
    }
}

impl fmt::Display for TraceFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02x}", self.0)
    }
}

/// Immutable context for a single span in a distributed trace.
///
/// Contains the W3C Trace Context fields: trace_id, span_id, trace_flags,
/// and optional tracestate/baggage key-value pairs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanContext {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    trace_flags: TraceFlags,
    trace_state: HashMap<String, String>,
    baggage: HashMap<String, String>,
}

impl SpanContext {
    /// Create a new root span context (no parent).
    pub fn new_root() -> Self {
        Self {
            trace_id: Self::generate_trace_id(),
            span_id: Self::generate_span_id(),
            parent_span_id: None,
            trace_flags: TraceFlags::SAMPLED,
            trace_state: HashMap::new(),
            baggage: HashMap::new(),
        }
    }

    /// Create a child span context inheriting trace_id from parent.
    pub fn new_child(parent: &SpanContext) -> Self {
        Self {
            trace_id: parent.trace_id.clone(),
            span_id: Self::generate_span_id(),
            parent_span_id: Some(parent.span_id.clone()),
            trace_flags: parent.trace_flags,
            trace_state: parent.trace_state.clone(),
            baggage: parent.baggage.clone(),
        }
    }

    /// Create from parsed W3C traceparent fields.
    pub(crate) fn from_w3c(trace_id: String, span_id: String, flags: TraceFlags) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: flags,
            trace_state: HashMap::new(),
            baggage: HashMap::new(),
        }
    }

    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    pub fn parent_span_id(&self) -> Option<&str> {
        self.parent_span_id.as_deref()
    }

    pub fn trace_flags(&self) -> TraceFlags {
        self.trace_flags
    }

    pub fn is_sampled(&self) -> bool {
        self.trace_flags.is_sampled()
    }

    pub fn trace_state(&self) -> &HashMap<String, String> {
        &self.trace_state
    }

    pub fn baggage(&self) -> &HashMap<String, String> {
        &self.baggage
    }

    pub fn with_trace_state(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.trace_state.insert(key.into(), value.into());
        self
    }

    pub fn with_baggage(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.baggage.insert(key.into(), value.into());
        self
    }

    fn generate_trace_id() -> String {
        let id = uuid::Uuid::new_v4();
        hex::encode(id.as_bytes())
    }

    fn generate_span_id() -> String {
        let id = uuid::Uuid::new_v4();
        hex::encode(&id.as_bytes()[..8])
    }
}

impl fmt::Display for SpanContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "trace_id={} span_id={} flags={}",
            self.trace_id, self.span_id, self.trace_flags
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_context() {
        let ctx = SpanContext::new_root();
        assert_eq!(ctx.trace_id().len(), 32); // 128-bit hex
        assert_eq!(ctx.span_id().len(), 16); // 64-bit hex
        assert!(ctx.parent_span_id().is_none());
        assert!(ctx.is_sampled());
    }

    #[test]
    fn test_child_context() {
        let parent = SpanContext::new_root();
        let child = SpanContext::new_child(&parent);

        assert_eq!(child.trace_id(), parent.trace_id());
        assert_ne!(child.span_id(), parent.span_id());
        assert_eq!(child.parent_span_id(), Some(parent.span_id()));
        assert_eq!(child.trace_flags(), parent.trace_flags());
    }

    #[test]
    fn test_baggage_propagation() {
        let parent = SpanContext::new_root().with_baggage("user.id", "u-123");

        let child = SpanContext::new_child(&parent);
        assert_eq!(child.baggage().get("user.id").unwrap(), "u-123");
    }

    #[test]
    fn test_trace_state() {
        let ctx = SpanContext::new_root()
            .with_trace_state("vendor1", "value1")
            .with_trace_state("vendor2", "value2");
        assert_eq!(ctx.trace_state().len(), 2);
    }

    #[test]
    fn test_trace_flags() {
        assert!(TraceFlags::SAMPLED.is_sampled());
        assert!(!TraceFlags::NONE.is_sampled());
        assert_eq!(format!("{}", TraceFlags::SAMPLED), "01");
    }
}
