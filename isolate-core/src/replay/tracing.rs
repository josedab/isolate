//! Distributed tracing and flamegraph generation for execution replay.
//!
//! Converts recorded execution events into OpenTelemetry-compatible trace
//! spans and generates flamegraph-compatible output for performance analysis.

use super::recording::{EventKind, Recording, RecordingEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// A unique trace identifier (compatible with W3C Trace Context).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct TraceId(pub String);

impl TraceId {
    /// Generate a new random trace ID (32-char hex).
    pub fn generate() -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        hasher.update(now.to_le_bytes());
        hasher.update(uuid::Uuid::new_v4().as_bytes());
        let hash = hex::encode(hasher.finalize());
        Self(hash[..32].to_string())
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A unique span identifier (16-char hex).
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpanId(pub String);

impl SpanId {
    /// Generate a new random span ID.
    pub fn generate() -> Self {
        let id = uuid::Uuid::new_v4();
        Self(hex::encode(&id.as_bytes()[..8]))
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Span status matching OTel conventions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpanStatus {
    /// The operation completed successfully.
    Ok,
    /// The operation encountered an error.
    Error(String),
    /// Status is not set.
    Unset,
}

/// A trace span representing an operation within a sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
    pub operation_name: String,
    pub service_name: String,
    pub start_time_us: u64,
    pub end_time_us: u64,
    pub status: SpanStatus,
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
}

impl TraceSpan {
    /// Duration of this span in microseconds.
    pub fn duration_us(&self) -> u64 {
        self.end_time_us.saturating_sub(self.start_time_us)
    }

    /// Self-time excluding child spans.
    pub fn self_time_us(&self, children: &[&TraceSpan]) -> u64 {
        let child_time: u64 = children.iter().map(|c| c.duration_us()).sum();
        self.duration_us().saturating_sub(child_time)
    }
}

/// An event within a span (log-like annotation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_us: u64,
    pub attributes: HashMap<String, String>,
}

/// A complete distributed trace for a sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub trace_id: TraceId,
    pub sandbox_id: String,
    pub root_span: SpanId,
    pub spans: Vec<TraceSpan>,
    pub total_duration_us: u64,
}

impl ExecutionTrace {
    /// Get the root span.
    pub fn root(&self) -> Option<&TraceSpan> {
        self.spans.iter().find(|s| s.span_id == self.root_span)
    }

    /// Get direct children of a span.
    pub fn children_of(&self, span_id: &SpanId) -> Vec<&TraceSpan> {
        self.spans.iter().filter(|s| s.parent_span_id.as_ref() == Some(span_id)).collect()
    }

    /// Get spans by operation name.
    pub fn spans_by_name(&self, name: &str) -> Vec<&TraceSpan> {
        self.spans.iter().filter(|s| s.operation_name == name).collect()
    }

    /// Get the critical path (longest chain of sequential spans).
    pub fn critical_path(&self) -> Vec<&TraceSpan> {
        fn longest_path<'a>(trace: &'a ExecutionTrace, span: &'a TraceSpan) -> Vec<&'a TraceSpan> {
            let children = trace.children_of(&span.span_id);
            if children.is_empty() {
                return vec![span];
            }
            let mut best = vec![span];
            for child in children {
                let mut path = vec![span];
                path.extend(longest_path(trace, child));
                if path.iter().map(|s| s.duration_us()).sum::<u64>()
                    > best.iter().map(|s| s.duration_us()).sum::<u64>()
                {
                    best = path;
                }
            }
            best
        }

        if let Some(root) = self.root() {
            longest_path(self, root)
        } else {
            vec![]
        }
    }
}

/// Converts a recording into a distributed trace.
pub struct TraceBuilder {
    service_name: String,
    trace_id: TraceId,
}

impl TraceBuilder {
    /// Create a new trace builder.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self { service_name: service_name.into(), trace_id: TraceId::generate() }
    }

    /// Use a specific trace ID (for correlation with external systems).
    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = trace_id;
        self
    }

    /// Build a trace from a recording.
    pub fn build_from_recording(&self, recording: &Recording) -> ExecutionTrace {
        let root_span_id = SpanId::generate();
        let mut spans = Vec::new();

        // Root span covering entire execution
        let exit_code = recording.exit_code().unwrap_or(-1);
        let status = if exit_code == 0 {
            SpanStatus::Ok
        } else {
            SpanStatus::Error(format!("exit code: {}", exit_code))
        };

        let mut root_attrs = HashMap::new();
        root_attrs.insert("sandbox.id".into(), recording.sandbox_id.clone());
        if let Some(ref hash) = recording.module_hash {
            root_attrs.insert("sandbox.module_hash".into(), hash.clone());
        }
        root_attrs.insert("sandbox.exit_code".into(), exit_code.to_string());

        let root_events = self.extract_span_events(&recording.events, 0, recording.duration_us);

        spans.push(TraceSpan {
            trace_id: self.trace_id.clone(),
            span_id: root_span_id.clone(),
            parent_span_id: None,
            operation_name: "sandbox.execute".into(),
            service_name: self.service_name.clone(),
            start_time_us: 0,
            end_time_us: recording.duration_us,
            status,
            attributes: root_attrs,
            events: root_events,
        });

        // Create child spans for grouped operations
        let phase_spans = self.build_phase_spans(recording, &root_span_id);
        spans.extend(phase_spans);

        ExecutionTrace {
            trace_id: self.trace_id.clone(),
            sandbox_id: recording.sandbox_id.clone(),
            root_span: root_span_id,
            spans,
            total_duration_us: recording.duration_us,
        }
    }

    /// Group events into logical phase spans (I/O, computation, filesystem, network).
    fn build_phase_spans(&self, recording: &Recording, parent_id: &SpanId) -> Vec<TraceSpan> {
        let mut spans = Vec::new();

        // Group consecutive I/O events
        let io_events: Vec<&RecordingEvent> = recording
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::Input(_) | EventKind::Output(_) | EventKind::ErrorOutput(_)
                )
            })
            .collect();

        if !io_events.is_empty() {
            let start = io_events.first().unwrap().timestamp_us;
            let end = io_events.last().unwrap().timestamp_us + 1;
            let mut attrs = HashMap::new();
            attrs.insert("io.event_count".into(), io_events.len().to_string());
            let total_bytes: usize = io_events
                .iter()
                .map(|e| match &e.kind {
                    EventKind::Input(d) | EventKind::Output(d) | EventKind::ErrorOutput(d) => {
                        d.len()
                    }
                    _ => 0,
                })
                .sum();
            attrs.insert("io.total_bytes".into(), total_bytes.to_string());

            spans.push(TraceSpan {
                trace_id: self.trace_id.clone(),
                span_id: SpanId::generate(),
                parent_span_id: Some(parent_id.clone()),
                operation_name: "sandbox.io".into(),
                service_name: self.service_name.clone(),
                start_time_us: start,
                end_time_us: end,
                status: SpanStatus::Ok,
                attributes: attrs,
                events: vec![],
            });
        }

        // Filesystem operations span
        let fs_events: Vec<&RecordingEvent> = recording
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::FileOp { .. }))
            .collect();

        if !fs_events.is_empty() {
            let start = fs_events.first().unwrap().timestamp_us;
            let end = fs_events.last().unwrap().timestamp_us + 1;
            let mut attrs = HashMap::new();
            attrs.insert("fs.operation_count".into(), fs_events.len().to_string());

            spans.push(TraceSpan {
                trace_id: self.trace_id.clone(),
                span_id: SpanId::generate(),
                parent_span_id: Some(parent_id.clone()),
                operation_name: "sandbox.filesystem".into(),
                service_name: self.service_name.clone(),
                start_time_us: start,
                end_time_us: end,
                status: SpanStatus::Ok,
                attributes: attrs,
                events: vec![],
            });
        }

        // Network operations span
        let net_events: Vec<&RecordingEvent> =
            recording.events.iter().filter(|e| matches!(e.kind, EventKind::NetOp { .. })).collect();

        if !net_events.is_empty() {
            let start = net_events.first().unwrap().timestamp_us;
            let end = net_events.last().unwrap().timestamp_us + 1;
            let mut attrs = HashMap::new();
            attrs.insert("net.operation_count".into(), net_events.len().to_string());

            spans.push(TraceSpan {
                trace_id: self.trace_id.clone(),
                span_id: SpanId::generate(),
                parent_span_id: Some(parent_id.clone()),
                operation_name: "sandbox.network".into(),
                service_name: self.service_name.clone(),
                start_time_us: start,
                end_time_us: end,
                status: SpanStatus::Ok,
                attributes: attrs,
                events: vec![],
            });
        }

        spans
    }

    fn extract_span_events(
        &self,
        events: &[RecordingEvent],
        start: u64,
        end: u64,
    ) -> Vec<SpanEvent> {
        events
            .iter()
            .filter(|e| e.timestamp_us >= start && e.timestamp_us <= end)
            .filter_map(|e| {
                let (name, attrs) = match &e.kind {
                    EventKind::MemorySnapshot { pages, used_bytes } => {
                        let mut a = HashMap::new();
                        a.insert("memory.pages".into(), pages.to_string());
                        a.insert("memory.used_bytes".into(), used_bytes.to_string());
                        ("memory.snapshot".into(), a)
                    }
                    EventKind::FuelCheckpoint(fuel) => {
                        let mut a = HashMap::new();
                        a.insert("fuel.consumed".into(), fuel.to_string());
                        ("fuel.checkpoint".into(), a)
                    }
                    _ => return None,
                };
                Some(SpanEvent { name, timestamp_us: e.timestamp_us, attributes: attrs })
            })
            .collect()
    }
}

/// A flamegraph frame for performance visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlamegraphFrame {
    /// Stack path (semicolon-delimited, e.g. "sandbox.execute;sandbox.io").
    pub stack: String,
    /// Time in this frame in microseconds.
    pub value: u64,
}

/// Generates folded stack format compatible with flamegraph tools.
pub struct FlamegraphGenerator;

impl FlamegraphGenerator {
    /// Generate folded stacks from an execution trace.
    ///
    /// Output is in the format used by Brendan Gregg's FlameGraph tools:
    /// `stack_a;stack_b;stack_c value`
    pub fn generate_folded(trace: &ExecutionTrace) -> Vec<FlamegraphFrame> {
        let mut frames = Vec::new();
        Self::collect_frames(trace, &trace.root_span, String::new(), &mut frames);
        frames
    }

    fn collect_frames(
        trace: &ExecutionTrace,
        span_id: &SpanId,
        parent_stack: String,
        frames: &mut Vec<FlamegraphFrame>,
    ) {
        let span = match trace.spans.iter().find(|s| s.span_id == *span_id) {
            Some(s) => s,
            None => return,
        };

        let stack = if parent_stack.is_empty() {
            span.operation_name.clone()
        } else {
            format!("{};{}", parent_stack, span.operation_name)
        };

        let children = trace.children_of(span_id);
        let child_time: u64 = children.iter().map(|c| c.duration_us()).sum();
        let self_time = span.duration_us().saturating_sub(child_time);

        if self_time > 0 {
            frames.push(FlamegraphFrame { stack: stack.clone(), value: self_time });
        }

        for child in children {
            Self::collect_frames(trace, &child.span_id, stack.clone(), frames);
        }
    }

    /// Render folded stacks as a string (one line per frame).
    pub fn render_folded(frames: &[FlamegraphFrame]) -> String {
        frames.iter().map(|f| format!("{} {}", f.stack, f.value)).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::recording::{EventKind, ExecutionRecorder};

    fn make_test_recording() -> Recording {
        let rec = ExecutionRecorder::new("trace-test");
        rec.record_event(EventKind::Input(b"hello".to_vec()));
        rec.record_event(EventKind::MemorySnapshot { pages: 4, used_bytes: 16384 });
        rec.record_event(EventKind::FileOp { path: "/data/input.txt".into(), op: "read".into() });
        rec.record_event(EventKind::Output(b"result".to_vec()));
        rec.record_event(EventKind::FuelCheckpoint(500_000));
        rec.record_event(EventKind::Exit(0));
        rec.finish()
    }

    #[test]
    fn test_trace_builder_basic() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("isolate-test");
        let trace = builder.build_from_recording(&recording);

        assert_eq!(trace.sandbox_id, "trace-test");
        assert!(!trace.spans.is_empty());

        let root = trace.root().unwrap();
        assert_eq!(root.operation_name, "sandbox.execute");
        assert_eq!(root.service_name, "isolate-test");
        assert!(root.parent_span_id.is_none());
    }

    #[test]
    fn test_trace_has_child_spans() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("isolate");
        let trace = builder.build_from_recording(&recording);

        let root = trace.root().unwrap();
        let children = trace.children_of(&root.span_id);

        // Should have io and fs child spans
        assert!(children.len() >= 2);
        let names: Vec<&str> = children.iter().map(|s| s.operation_name.as_str()).collect();
        assert!(names.contains(&"sandbox.io"));
        assert!(names.contains(&"sandbox.filesystem"));
    }

    #[test]
    fn test_trace_with_custom_trace_id() {
        let recording = make_test_recording();
        let custom_id = TraceId("abcdef0123456789abcdef0123456789".into());
        let builder = TraceBuilder::new("svc").with_trace_id(custom_id.clone());
        let trace = builder.build_from_recording(&recording);
        assert_eq!(trace.trace_id, custom_id);
    }

    #[test]
    fn test_trace_error_status() {
        let rec = ExecutionRecorder::new("err-test");
        rec.record_event(EventKind::Exit(1));
        let recording = rec.finish();

        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);
        let root = trace.root().unwrap();
        assert!(matches!(root.status, SpanStatus::Error(_)));
    }

    #[test]
    fn test_trace_ok_status() {
        let rec = ExecutionRecorder::new("ok-test");
        rec.record_event(EventKind::Exit(0));
        let recording = rec.finish();

        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);
        let root = trace.root().unwrap();
        assert_eq!(root.status, SpanStatus::Ok);
    }

    #[test]
    fn test_span_duration() {
        let span = TraceSpan {
            trace_id: TraceId("t".into()),
            span_id: SpanId("s".into()),
            parent_span_id: None,
            operation_name: "test".into(),
            service_name: "svc".into(),
            start_time_us: 100,
            end_time_us: 500,
            status: SpanStatus::Ok,
            attributes: HashMap::new(),
            events: vec![],
        };
        assert_eq!(span.duration_us(), 400);
    }

    #[test]
    fn test_spans_by_name() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        let io_spans = trace.spans_by_name("sandbox.io");
        assert_eq!(io_spans.len(), 1);
    }

    #[test]
    fn test_critical_path() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        let path = trace.critical_path();
        assert!(!path.is_empty());
        assert_eq!(path[0].operation_name, "sandbox.execute");
    }

    #[test]
    fn test_flamegraph_generation() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        let frames = FlamegraphGenerator::generate_folded(&trace);
        assert!(!frames.is_empty());

        // Should have root self-time frame
        assert!(frames.iter().any(|f| f.stack.starts_with("sandbox.execute")));

        let rendered = FlamegraphGenerator::render_folded(&frames);
        assert!(!rendered.is_empty());
        // Each line: "stack value"
        for line in rendered.lines() {
            let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
            assert_eq!(parts.len(), 2);
            assert!(parts[0].parse::<u64>().is_ok());
        }
    }

    #[test]
    fn test_flamegraph_stack_paths() {
        let recording = make_test_recording();
        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        let frames = FlamegraphGenerator::generate_folded(&trace);
        let stacks: Vec<&str> = frames.iter().map(|f| f.stack.as_str()).collect();

        // Child spans should have semicolon-delimited paths
        assert!(stacks.iter().any(|s| s.contains("sandbox.execute;sandbox.io")));
    }

    #[test]
    fn test_empty_recording_trace() {
        let rec = ExecutionRecorder::new("empty");
        let recording = rec.finish();
        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        assert_eq!(trace.spans.len(), 1); // root only
        let root = trace.root().unwrap();
        assert_eq!(root.operation_name, "sandbox.execute");
    }

    #[test]
    fn test_network_span_creation() {
        let rec = ExecutionRecorder::new("net-test");
        rec.record_event(EventKind::NetOp {
            host: "api.example.com".into(),
            port: 443,
            op: "connect".into(),
        });
        rec.record_event(EventKind::Exit(0));
        let recording = rec.finish();

        let builder = TraceBuilder::new("svc");
        let trace = builder.build_from_recording(&recording);

        let net_spans = trace.spans_by_name("sandbox.network");
        assert_eq!(net_spans.len(), 1);
    }
}
