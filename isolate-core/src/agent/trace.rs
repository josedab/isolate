//! Execution traces for AI agent tool calls.
//!
//! Captures call graphs, timing, resource usage, and I/O for each tool
//! invocation within an agent session. Traces enable debugging, auditing,
//! and cost attribution for LLM-driven workflows.

use super::types::ResourceUsageSummary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// Complete execution trace for a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Unique trace ID.
    pub trace_id: Uuid,
    /// Session ID this trace belongs to.
    pub session_id: Uuid,
    /// Root span of the execution.
    pub root_span: TraceSpan,
    /// Total wall time for the entire trace.
    pub total_duration: Duration,
    /// Total fuel consumed across all spans.
    pub total_fuel: u64,
    /// Per-call resource budget that was enforced.
    pub resource_budget: ResourceBudget,
    /// Whether the budget was exceeded.
    pub budget_exceeded: bool,
    /// Structured input provided to the tool.
    pub input_summary: InputSummary,
    /// Structured output from the tool.
    pub output_summary: OutputSummary,
}

/// A span within an execution trace (forms a tree/call graph).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    /// Span name (function/tool name).
    pub name: String,
    /// Span kind.
    pub kind: SpanKind,
    /// Start offset from trace start.
    pub start_offset: Duration,
    /// Duration of this span.
    pub duration: Duration,
    /// Resource usage within this span.
    pub resource_usage: ResourceUsageSummary,
    /// Key-value attributes.
    pub attributes: HashMap<String, String>,
    /// Child spans.
    pub children: Vec<TraceSpan>,
    /// Status of this span.
    pub status: SpanStatus,
}

impl TraceSpan {
    /// Create a new trace span.
    pub fn new(name: impl Into<String>, kind: SpanKind) -> Self {
        Self {
            name: name.into(),
            kind,
            start_offset: Duration::ZERO,
            duration: Duration::ZERO,
            resource_usage: ResourceUsageSummary::default(),
            attributes: HashMap::new(),
            children: Vec::new(),
            status: SpanStatus::Ok,
        }
    }

    /// Set an attribute on this span.
    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set status.
    pub fn with_status(mut self, status: SpanStatus) -> Self {
        self.status = status;
        self
    }

    /// Add a child span.
    pub fn with_child(mut self, child: TraceSpan) -> Self {
        self.children.push(child);
        self
    }

    /// Get total depth of the span tree.
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }

    /// Count total spans in the tree.
    pub fn span_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.span_count()).sum::<usize>()
    }
}

/// Kind of trace span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Top-level tool execution.
    ToolCall,
    /// WASM sandbox creation.
    SandboxCreate,
    /// WASM module compilation.
    Compilation,
    /// WASM execution.
    Execution,
    /// I/O operation.
    Io,
    /// Capability check.
    CapabilityCheck,
    /// Custom span.
    Custom(String),
}

/// Status of a trace span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    /// Completed successfully.
    Ok,
    /// Completed with an error.
    Error(String),
    /// Timed out.
    Timeout,
    /// Cancelled.
    Cancelled,
}

/// Per-call resource budget for agent tool invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// Maximum fuel for this call.
    pub max_fuel: Option<u64>,
    /// Maximum memory in bytes.
    pub max_memory_bytes: Option<usize>,
    /// Maximum wall time.
    pub max_wall_time: Option<Duration>,
    /// Maximum output bytes.
    pub max_output_bytes: Option<usize>,
    /// Maximum I/O operations.
    pub max_io_ops: Option<u64>,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_fuel: Some(10_000_000),
            max_memory_bytes: Some(64 * 1024 * 1024),
            max_wall_time: Some(Duration::from_secs(30)),
            max_output_bytes: Some(1024 * 1024),
            max_io_ops: None,
        }
    }
}

impl ResourceBudget {
    /// Create a budget with specific fuel limit.
    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.max_fuel = Some(fuel);
        self
    }

    /// Create a budget with specific memory limit.
    pub fn with_memory(mut self, bytes: usize) -> Self {
        self.max_memory_bytes = Some(bytes);
        self
    }

    /// Create a budget with specific wall time.
    pub fn with_wall_time(mut self, duration: Duration) -> Self {
        self.max_wall_time = Some(duration);
        self
    }

    /// Check if actual usage exceeds this budget.
    pub fn is_exceeded(&self, usage: &ResourceUsageSummary) -> bool {
        if let Some(max_fuel) = self.max_fuel {
            if usage.fuel_consumed > max_fuel {
                return true;
            }
        }
        if let Some(max_memory) = self.max_memory_bytes {
            if usage.peak_memory_bytes > max_memory {
                return true;
            }
        }
        false
    }
}

/// Summary of input provided to a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSummary {
    /// Size of the input in bytes.
    pub size_bytes: usize,
    /// Content type.
    pub content_type: String,
    /// Schema of the JSON input (top-level keys).
    pub schema_keys: Vec<String>,
}

impl InputSummary {
    /// Create from a JSON value.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let size_bytes = serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0);
        let schema_keys = match value {
            serde_json::Value::Object(map) => map.keys().cloned().collect(),
            _ => Vec::new(),
        };
        Self { size_bytes, content_type: "application/json".to_string(), schema_keys }
    }
}

/// Summary of output from a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSummary {
    /// Size of stdout in bytes.
    pub stdout_bytes: usize,
    /// Size of stderr in bytes.
    pub stderr_bytes: usize,
    /// Whether output was valid JSON.
    pub is_json: bool,
    /// Exit code.
    pub exit_code: i32,
    /// Whether output was truncated.
    pub was_truncated: bool,
}

/// Builder for constructing execution traces.
pub struct TraceBuilder {
    session_id: Uuid,
    trace_id: Uuid,
    budget: ResourceBudget,
    spans: Vec<TraceSpan>,
    start: std::time::Instant,
}

impl TraceBuilder {
    /// Start building a new trace.
    pub fn new(session_id: Uuid) -> Self {
        Self {
            session_id,
            trace_id: Uuid::new_v4(),
            budget: ResourceBudget::default(),
            spans: Vec::new(),
            start: std::time::Instant::now(),
        }
    }

    /// Set the resource budget.
    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Record a span.
    pub fn record_span(&mut self, span: TraceSpan) {
        self.spans.push(span);
    }

    /// Finish the trace and build the result.
    pub fn finish(
        self,
        tool_name: &str,
        input: &serde_json::Value,
        output: &super::types::CodeExecutionResult,
    ) -> ExecutionTrace {
        let total_duration = self.start.elapsed();
        let total_fuel = output.resource_usage.fuel_consumed;

        let root_span = TraceSpan::new(tool_name, SpanKind::ToolCall)
            .with_duration(total_duration)
            .with_status(if output.success() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error(output.stderr.clone())
            })
            .with_attr("exit_code", output.exit_code.to_string());

        // Nest recorded spans as children of root
        let root_span = self.spans.into_iter().fold(root_span, |s, child| s.with_child(child));

        let budget_exceeded = self.budget.is_exceeded(&output.resource_usage);

        ExecutionTrace {
            trace_id: self.trace_id,
            session_id: self.session_id,
            root_span,
            total_duration,
            total_fuel,
            resource_budget: self.budget,
            budget_exceeded,
            input_summary: InputSummary::from_json(input),
            output_summary: OutputSummary {
                stdout_bytes: output.stdout.len(),
                stderr_bytes: output.stderr.len(),
                is_json: serde_json::from_str::<serde_json::Value>(&output.stdout).is_ok(),
                exit_code: output.exit_code,
                was_truncated: output.status == super::types::ExecutionStatus::OutputTruncated,
            },
        }
    }
}

/// Collection of traces for a session, with query capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceStore {
    traces: Vec<ExecutionTrace>,
}

impl TraceStore {
    /// Create an empty trace store.
    pub fn new() -> Self {
        Self { traces: Vec::new() }
    }

    /// Add a trace.
    pub fn push(&mut self, trace: ExecutionTrace) {
        self.traces.push(trace);
    }

    /// Get all traces.
    pub fn all(&self) -> &[ExecutionTrace] {
        &self.traces
    }

    /// Get traces for a specific tool.
    pub fn for_tool(&self, tool_name: &str) -> Vec<&ExecutionTrace> {
        self.traces.iter().filter(|t| t.root_span.name == tool_name).collect()
    }

    /// Get traces that exceeded their budget.
    pub fn budget_exceeded(&self) -> Vec<&ExecutionTrace> {
        self.traces.iter().filter(|t| t.budget_exceeded).collect()
    }

    /// Total fuel consumed across all traces.
    pub fn total_fuel(&self) -> u64 {
        self.traces.iter().map(|t| t.total_fuel).sum()
    }

    /// Total wall time across all traces.
    pub fn total_duration(&self) -> Duration {
        self.traces.iter().map(|t| t.total_duration).sum()
    }

    /// Number of traces.
    pub fn len(&self) -> usize {
        self.traces.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.traces.is_empty()
    }

    /// Get summary statistics.
    pub fn stats(&self) -> TraceStats {
        let total = self.traces.len();
        let succeeded = self.traces.iter().filter(|t| !t.budget_exceeded).count();
        let avg_duration =
            if total > 0 { self.total_duration() / total as u32 } else { Duration::ZERO };

        TraceStats {
            total_traces: total,
            succeeded,
            budget_exceeded: total - succeeded,
            total_fuel: self.total_fuel(),
            avg_duration,
        }
    }
}

/// Summary statistics for a trace store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStats {
    pub total_traces: usize,
    pub succeeded: usize,
    pub budget_exceeded: usize,
    pub total_fuel: u64,
    pub avg_duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_span_creation() {
        let span = TraceSpan::new("test-tool", SpanKind::ToolCall)
            .with_duration(Duration::from_millis(100))
            .with_attr("key", "value")
            .with_status(SpanStatus::Ok);

        assert_eq!(span.name, "test-tool");
        assert_eq!(span.duration, Duration::from_millis(100));
        assert_eq!(span.attributes.get("key"), Some(&"value".to_string()));
        assert_eq!(span.depth(), 1);
        assert_eq!(span.span_count(), 1);
    }

    #[test]
    fn test_trace_span_tree() {
        let child1 = TraceSpan::new("compile", SpanKind::Compilation)
            .with_duration(Duration::from_millis(10));
        let child2 =
            TraceSpan::new("execute", SpanKind::Execution).with_duration(Duration::from_millis(50));

        let root =
            TraceSpan::new("tool-call", SpanKind::ToolCall).with_child(child1).with_child(child2);

        assert_eq!(root.depth(), 2);
        assert_eq!(root.span_count(), 3);
    }

    #[test]
    fn test_resource_budget_defaults() {
        let budget = ResourceBudget::default();
        assert_eq!(budget.max_fuel, Some(10_000_000));
        assert_eq!(budget.max_memory_bytes, Some(64 * 1024 * 1024));
    }

    #[test]
    fn test_resource_budget_exceeded() {
        let budget = ResourceBudget::default().with_fuel(1000);
        let usage = ResourceUsageSummary { fuel_consumed: 2000, ..Default::default() };
        assert!(budget.is_exceeded(&usage));

        let within = ResourceUsageSummary { fuel_consumed: 500, ..Default::default() };
        assert!(!budget.is_exceeded(&within));
    }

    #[test]
    fn test_input_summary_from_json() {
        let input = serde_json::json!({"query": "hello", "count": 5});
        let summary = InputSummary::from_json(&input);

        assert_eq!(summary.content_type, "application/json");
        assert!(summary.schema_keys.contains(&"query".to_string()));
        assert!(summary.schema_keys.contains(&"count".to_string()));
        assert!(summary.size_bytes > 0);
    }

    #[test]
    fn test_trace_store() {
        let mut store = TraceStore::new();
        assert!(store.is_empty());

        let trace = ExecutionTrace {
            trace_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            root_span: TraceSpan::new("test-tool", SpanKind::ToolCall),
            total_duration: Duration::from_millis(100),
            total_fuel: 5000,
            resource_budget: ResourceBudget::default(),
            budget_exceeded: false,
            input_summary: InputSummary {
                size_bytes: 10,
                content_type: "application/json".to_string(),
                schema_keys: vec!["key".to_string()],
            },
            output_summary: OutputSummary {
                stdout_bytes: 20,
                stderr_bytes: 0,
                is_json: true,
                exit_code: 0,
                was_truncated: false,
            },
        };

        store.push(trace);
        assert_eq!(store.len(), 1);
        assert_eq!(store.total_fuel(), 5000);
        assert!(store.budget_exceeded().is_empty());
        assert_eq!(store.for_tool("test-tool").len(), 1);
        assert!(store.for_tool("other-tool").is_empty());
    }

    #[test]
    fn test_trace_stats() {
        let store = TraceStore::new();
        let stats = store.stats();
        assert_eq!(stats.total_traces, 0);
        assert_eq!(stats.total_fuel, 0);
    }
}
