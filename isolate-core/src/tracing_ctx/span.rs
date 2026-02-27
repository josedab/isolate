use super::context::SpanContext;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// The type of span (client, server, internal, producer, consumer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

/// Span completion status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanStatus {
    Ok,
    Error(String),
    Unset,
}

/// A recorded event within a span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_ns: u64,
    pub attributes: Vec<(String, String)>,
}

/// A completed span representing a unit of work within a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub name: String,
    pub context: SpanContext,
    pub kind: SpanKind,
    pub status: SpanStatus,
    pub start_epoch_ns: u64,
    pub duration_ns: u64,
    pub attributes: Vec<(String, String)>,
    pub events: Vec<SpanEvent>,
}

/// Builder for creating spans with fluent API.
pub struct SpanBuilder {
    name: String,
    parent: Option<SpanContext>,
    kind: SpanKind,
    attributes: Vec<(String, String)>,
    events: Vec<SpanEvent>,
    start: Instant,
}

impl SpanBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent: None,
            kind: SpanKind::Internal,
            attributes: Vec::new(),
            events: Vec::new(),
            start: Instant::now(),
        }
    }

    pub fn with_parent(mut self, parent: &SpanContext) -> Self {
        self.parent = Some(parent.clone());
        self
    }

    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    pub fn with_event(mut self, event: SpanEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Build a completed span snapshot.
    pub fn build(self) -> Span {
        let context = match &self.parent {
            Some(parent) => SpanContext::new_child(parent),
            None => SpanContext::new_root(),
        };

        let now_epoch_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let duration = self.start.elapsed();

        Span {
            name: self.name,
            context,
            kind: self.kind,
            status: SpanStatus::Unset,
            start_epoch_ns: now_epoch_ns.saturating_sub(duration.as_nanos() as u64),
            duration_ns: duration.as_nanos() as u64,
            attributes: self.attributes,
            events: self.events,
        }
    }

    /// Build a span and immediately mark it with a status.
    pub fn build_with_status(self, status: SpanStatus) -> Span {
        let mut span = self.build();
        span.status = status;
        span
    }
}

/// Collects finished spans for batch export.
///
/// Thread-safe; designed for use across async tasks.
pub struct SpanRecorder {
    spans: parking_lot::Mutex<Vec<Span>>,
    max_spans: usize,
}

impl SpanRecorder {
    pub fn new(max_spans: usize) -> Self {
        Self { spans: parking_lot::Mutex::new(Vec::with_capacity(max_spans.min(256))), max_spans }
    }

    /// Record a completed span.
    pub fn record(&self, span: Span) {
        let mut spans = self.spans.lock();
        if spans.len() >= self.max_spans {
            let half = spans.len() / 2;
            spans.drain(..half);
        }
        spans.push(span);
    }

    /// Drain all recorded spans for export.
    pub fn drain(&self) -> Vec<Span> {
        let mut spans = self.spans.lock();
        std::mem::take(&mut *spans)
    }

    /// Number of buffered spans.
    pub fn len(&self) -> usize {
        self.spans.lock().len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.spans.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_builder_root() {
        let span = SpanBuilder::new("root.operation").build();
        assert_eq!(span.name, "root.operation");
        assert!(span.context.parent_span_id().is_none());
        assert!(matches!(span.kind, SpanKind::Internal));
    }

    #[test]
    fn test_span_builder_with_parent() {
        let parent = SpanContext::new_root();
        let span = SpanBuilder::new("child.operation")
            .with_parent(&parent)
            .with_kind(SpanKind::Server)
            .build();

        assert_eq!(span.context.trace_id(), parent.trace_id());
        assert_eq!(span.context.parent_span_id(), Some(parent.span_id()));
        assert!(matches!(span.kind, SpanKind::Server));
    }

    #[test]
    fn test_span_builder_attributes() {
        let span = SpanBuilder::new("op")
            .with_attribute("sandbox.id", "sb-1")
            .with_attribute("tenant.id", "t-1")
            .build();
        assert_eq!(span.attributes.len(), 2);
    }

    #[test]
    fn test_span_with_status() {
        let span = SpanBuilder::new("op").build_with_status(SpanStatus::Ok);
        assert!(matches!(span.status, SpanStatus::Ok));

        let span = SpanBuilder::new("op").build_with_status(SpanStatus::Error("timeout".into()));
        assert!(matches!(span.status, SpanStatus::Error(_)));
    }

    #[test]
    fn test_span_recorder() {
        let recorder = SpanRecorder::new(10);
        assert!(recorder.is_empty());

        recorder.record(SpanBuilder::new("op1").build());
        recorder.record(SpanBuilder::new("op2").build());
        assert_eq!(recorder.len(), 2);

        let spans = recorder.drain();
        assert_eq!(spans.len(), 2);
        assert!(recorder.is_empty());
    }

    #[test]
    fn test_span_recorder_eviction() {
        let recorder = SpanRecorder::new(4);
        for i in 0..6 {
            recorder.record(SpanBuilder::new(format!("op-{i}")).build());
        }
        assert!(recorder.len() <= 4);
    }

    #[test]
    fn test_span_event() {
        let event = SpanEvent {
            name: "exception".into(),
            timestamp_ns: 12345,
            attributes: vec![("error.message".into(), "oom".into())],
        };
        let span = SpanBuilder::new("op").with_event(event).build();
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "exception");
    }
}
