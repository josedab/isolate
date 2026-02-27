//! Agent session management with execution history and budgets.

use super::tools::ToolRegistry;
use super::trace::{ResourceBudget, SpanKind, SpanStatus, TraceBuilder, TraceSpan, TraceStore};
use super::types::*;
use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::engine::WasmEngine;
use crate::error::{Error, Result};
use crate::sandbox::Sandbox;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// A stateful session for AI agent code execution.
///
/// Tracks execution history, enforces budgets, and manages tool access.
pub struct AgentSession {
    /// Unique session identifier.
    id: Uuid,
    /// Session configuration.
    config: AgentConfig,
    /// Registered tools.
    tools: ToolRegistry,
    /// Shared WASM engine for module caching.
    engine: Arc<WasmEngine>,
    /// Execution history.
    history: Vec<ExecutionRecord>,
    /// Total fuel consumed across all executions.
    total_fuel_consumed: u64,
    /// Number of tool calls made.
    tool_call_count: usize,
    /// Execution traces.
    traces: TraceStore,
    /// Session creation time.
    created_at: DateTime<Utc>,
}

impl AgentSession {
    /// Create a new agent session.
    pub fn new(config: AgentConfig) -> Self {
        let engine = Arc::new(WasmEngine::new().expect("failed to create WASM engine"));
        Self::with_engine(config, engine)
    }

    /// Create a new agent session with a shared engine.
    pub fn with_engine(config: AgentConfig, engine: Arc<WasmEngine>) -> Self {
        Self {
            id: Uuid::new_v4(),
            config,
            tools: ToolRegistry::new(),
            engine,
            history: Vec::new(),
            total_fuel_consumed: 0,
            tool_call_count: 0,
            traces: TraceStore::new(),
            created_at: Utc::now(),
        }
    }

    /// Get the session ID.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the session configuration.
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Get the tool registry.
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Register a tool for this session.
    pub fn register_tool(&mut self, tool: super::tools::ToolDefinition) {
        self.tools.register(tool);
    }

    /// Get the execution history.
    pub fn history(&self) -> &[ExecutionRecord] {
        &self.history
    }

    /// Get total fuel consumed across all executions.
    pub fn total_fuel_consumed(&self) -> u64 {
        self.total_fuel_consumed
    }

    /// Get the number of tool calls made.
    pub fn tool_call_count(&self) -> usize {
        self.tool_call_count
    }

    /// Get remaining fuel budget.
    pub fn remaining_fuel(&self) -> Option<u64> {
        self.config.fuel_budget.map(|budget| budget.saturating_sub(self.total_fuel_consumed))
    }

    /// Check if the session has exceeded its tool call limit.
    pub fn is_tool_limit_reached(&self) -> bool {
        self.tool_call_count >= self.config.max_tool_calls
    }

    /// Get the execution trace store.
    pub fn traces(&self) -> &TraceStore {
        &self.traces
    }

    /// Execute a code request in the sandbox.
    pub async fn execute(&mut self, request: CodeExecutionRequest) -> Result<CodeExecutionResult> {
        // Check tool call budget
        if self.is_tool_limit_reached() {
            return Err(Error::Execution(format!(
                "Tool call limit exceeded: {} >= {}",
                self.tool_call_count, self.config.max_tool_calls
            )));
        }

        // Check fuel budget
        if let Some(remaining) = self.remaining_fuel() {
            if remaining == 0 {
                return Err(Error::FuelExhausted { limit: self.config.fuel_budget.unwrap_or(0) });
            }
        }

        // Validate tool if this is a tool call
        if let Some(ref tool_name) = request.tool_name {
            if !self.tools.has(tool_name) {
                return Err(Error::Execution(format!("Unknown tool: {}", tool_name)));
            }
        }

        // Determine execution limits
        let fuel = self.remaining_fuel().unwrap_or(10_000_000);
        let timeout = if let Some(ref tool_name) = request.tool_name {
            self.tools
                .get(tool_name)
                .and_then(|t| t.timeout_secs)
                .map(Duration::from_secs)
                .unwrap_or(self.config.execution_timeout)
        } else {
            self.config.execution_timeout
        };

        // Build sandbox config
        let input_json = serde_json::to_vec(&request.input)
            .map_err(|e| Error::Execution(format!("Failed to serialize input: {}", e)))?;

        let mut builder = SandboxConfig::builder()
            .module(&request.module_bytes)?
            .memory_limit(self.config.memory_limit)
            .fuel(fuel)
            .wall_time_limit(timeout)
            .capability(Capability::stdout())
            .capability(Capability::stdin());

        if self.config.capture_stderr {
            builder = builder.capability(Capability::stderr());
        }

        // Add session environment variables
        for (key, value) in &self.config.env {
            builder = builder.env(key, value);
        }

        // Add request metadata as env vars with ISOLATE_AGENT_ prefix
        for (key, value) in &request.metadata {
            builder = builder.env(format!("ISOLATE_AGENT_{}", key.to_uppercase()), value);
        }

        let config = builder.build()?;

        // Build per-call resource budget
        let budget = ResourceBudget::default()
            .with_fuel(fuel)
            .with_memory(self.config.memory_limit)
            .with_wall_time(timeout);

        // Start trace
        let mut trace_builder = TraceBuilder::new(self.id).with_budget(budget);

        // Record compilation span
        let compile_start = std::time::Instant::now();

        // Execute
        let mut sandbox = Sandbox::create_with_engine(config, self.engine.clone()).await?;

        trace_builder.record_span(
            TraceSpan::new("sandbox_create", SpanKind::SandboxCreate)
                .with_duration(compile_start.elapsed()),
        );

        let exec_start = std::time::Instant::now();
        let output = sandbox.run(&input_json).await;

        let result = match output {
            Ok(output) => {
                let stdout = output.stdout_str();
                let stderr = output.stderr_str();

                // Parse stdout as JSON if possible, otherwise use as string
                let parsed_output = serde_json::from_str::<serde_json::Value>(&stdout)
                    .unwrap_or_else(|_| serde_json::Value::String(stdout.clone()));

                // Check if output exceeds size limit
                let (status, final_output) = if stdout.len() > self.config.max_output_size {
                    let truncated = &stdout[..self.config.max_output_size];
                    (
                        ExecutionStatus::OutputTruncated,
                        serde_json::Value::String(truncated.to_string()),
                    )
                } else if output.exit_code == 0 {
                    (ExecutionStatus::Success, parsed_output)
                } else {
                    (ExecutionStatus::Failed, parsed_output)
                };

                // Track fuel
                let fuel_used = output.resource_usage.fuel_consumed;
                self.total_fuel_consumed += fuel_used;

                CodeExecutionResult {
                    status,
                    output: final_output,
                    stdout,
                    stderr,
                    exit_code: output.exit_code,
                    duration: output.duration,
                    resource_usage: output.resource_usage.into(),
                    tool_name: request.tool_name.clone(),
                }
            }
            Err(ref e) if e.is_timeout() => CodeExecutionResult {
                status: ExecutionStatus::Timeout,
                output: serde_json::Value::Null,
                stdout: String::new(),
                stderr: format!("Execution timed out: {}", e),
                exit_code: -1,
                duration: timeout,
                resource_usage: ResourceUsageSummary::default(),
                tool_name: request.tool_name.clone(),
            },
            Err(ref e) if e.is_resource_limit() => CodeExecutionResult {
                status: ExecutionStatus::ResourceExceeded,
                output: serde_json::Value::Null,
                stdout: String::new(),
                stderr: format!("Resource limit exceeded: {}", e),
                exit_code: -1,
                duration: Duration::ZERO,
                resource_usage: ResourceUsageSummary::default(),
                tool_name: request.tool_name.clone(),
            },
            Err(e) => return Err(e),
        };

        // Record execution span and finish trace
        let exec_status = if result.success() {
            SpanStatus::Ok
        } else {
            SpanStatus::Error(result.stderr.clone())
        };
        trace_builder.record_span(
            TraceSpan::new("wasm_execution", SpanKind::Execution)
                .with_duration(exec_start.elapsed())
                .with_status(exec_status),
        );

        let tool_name_for_trace = request.tool_name.as_deref().unwrap_or("anonymous");
        let trace = trace_builder.finish(tool_name_for_trace, &request.input, &result);
        self.traces.push(trace);

        // Record execution
        self.tool_call_count += 1;
        self.history.push(ExecutionRecord {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            request_input: request.input,
            tool_name: request.tool_name,
            status: result.status.clone(),
            exit_code: result.exit_code,
            duration: result.duration,
            fuel_consumed: result.resource_usage.fuel_consumed,
        });

        Ok(result)
    }

    /// Get a summary of the session.
    pub fn summary(&self) -> SessionSummary {
        let successful =
            self.history.iter().filter(|r| r.status == ExecutionStatus::Success).count();
        let failed = self.history.len() - successful;
        let total_duration: Duration = self.history.iter().map(|r| r.duration).sum();

        SessionSummary {
            session_id: self.id,
            total_executions: self.history.len(),
            successful_executions: successful,
            failed_executions: failed,
            total_fuel_consumed: self.total_fuel_consumed,
            remaining_fuel: self.remaining_fuel(),
            total_duration,
            tool_calls_remaining: self.config.max_tool_calls.saturating_sub(self.tool_call_count),
            created_at: self.created_at,
        }
    }
}

/// Record of a single execution within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique record ID.
    pub id: Uuid,
    /// When the execution occurred.
    pub timestamp: DateTime<Utc>,
    /// Input data that was provided.
    pub request_input: serde_json::Value,
    /// Tool name if this was a tool call.
    pub tool_name: Option<String>,
    /// Execution status.
    pub status: ExecutionStatus,
    /// Exit code.
    pub exit_code: i32,
    /// Execution duration.
    pub duration: Duration,
    /// Fuel consumed by this execution.
    pub fuel_consumed: u64,
}

/// Summary of an agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session ID.
    pub session_id: Uuid,
    /// Total number of executions.
    pub total_executions: usize,
    /// Number of successful executions.
    pub successful_executions: usize,
    /// Number of failed executions.
    pub failed_executions: usize,
    /// Total fuel consumed.
    pub total_fuel_consumed: u64,
    /// Remaining fuel budget.
    pub remaining_fuel: Option<u64>,
    /// Total duration of all executions.
    pub total_duration: Duration,
    /// Remaining tool calls.
    pub tool_calls_remaining: usize,
    /// Session creation time.
    pub created_at: DateTime<Utc>,
}

/// Serializable session snapshot for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Session ID.
    pub id: Uuid,
    /// Session configuration.
    pub config: AgentConfig,
    /// Execution history.
    pub history: Vec<ExecutionRecord>,
    /// Total fuel consumed.
    pub total_fuel_consumed: u64,
    /// Tool call count.
    pub tool_call_count: usize,
    /// Session creation time.
    pub created_at: DateTime<Utc>,
    /// Snapshot creation time.
    pub snapshot_at: DateTime<Utc>,
}

impl AgentSession {
    /// Create a persistent snapshot of this session.
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            id: self.id,
            config: self.config.clone(),
            history: self.history.clone(),
            total_fuel_consumed: self.total_fuel_consumed,
            tool_call_count: self.tool_call_count,
            created_at: self.created_at,
            snapshot_at: Utc::now(),
        }
    }

    /// Restore a session from a snapshot.
    pub fn from_snapshot(snapshot: SessionSnapshot) -> Self {
        let engine = Arc::new(WasmEngine::new().expect("failed to create WASM engine"));
        Self::from_snapshot_with_engine(snapshot, engine)
    }

    /// Restore a session from a snapshot with a shared engine.
    pub fn from_snapshot_with_engine(snapshot: SessionSnapshot, engine: Arc<WasmEngine>) -> Self {
        Self {
            id: snapshot.id,
            config: snapshot.config,
            tools: ToolRegistry::new(),
            engine,
            history: snapshot.history,
            total_fuel_consumed: snapshot.total_fuel_consumed,
            tool_call_count: snapshot.tool_call_count,
            traces: TraceStore::new(),
            created_at: snapshot.created_at,
        }
    }

    /// Serialize this session to JSON for persistence.
    pub fn save(&self) -> Result<String> {
        let snapshot = self.snapshot();
        serde_json::to_string_pretty(&snapshot)
            .map_err(|e| Error::Execution(format!("Failed to serialize session: {}", e)))
    }

    /// Restore a session from a JSON string.
    pub fn load(json: &str) -> Result<Self> {
        let snapshot: SessionSnapshot = serde_json::from_str(json)
            .map_err(|e| Error::Execution(format!("Failed to deserialize session: {}", e)))?;
        Ok(Self::from_snapshot(snapshot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::ToolDefinition;

    #[test]
    fn test_session_creation() {
        let config = AgentConfig::builder()
            .memory_limit(64 * 1024 * 1024)
            .max_tool_calls(10)
            .fuel_budget(1_000_000)
            .build();

        let session = AgentSession::new(config);
        assert_eq!(session.tool_call_count(), 0);
        assert_eq!(session.remaining_fuel(), Some(1_000_000));
        assert!(!session.is_tool_limit_reached());
    }

    #[test]
    fn test_session_tool_registration() {
        let mut session = AgentSession::new(AgentConfig::default());
        assert!(session.tools().is_empty());

        session.register_tool(ToolDefinition::code_execute());
        assert_eq!(session.tools().len(), 1);
        assert!(session.tools().has("code_execute"));
    }

    #[test]
    fn test_session_summary() {
        let session = AgentSession::new(AgentConfig::default());
        let summary = session.summary();

        assert_eq!(summary.total_executions, 0);
        assert_eq!(summary.successful_executions, 0);
        assert_eq!(summary.failed_executions, 0);
        assert_eq!(summary.total_fuel_consumed, 0);
    }

    #[test]
    fn test_session_snapshot_roundtrip() {
        let config = AgentConfig::builder()
            .memory_limit(64 * 1024 * 1024)
            .max_tool_calls(10)
            .fuel_budget(1_000_000)
            .build();

        let session = AgentSession::new(config);
        let original_id = session.id();

        let snapshot = session.snapshot();
        assert_eq!(snapshot.id, original_id);
        assert_eq!(snapshot.total_fuel_consumed, 0);

        let restored = AgentSession::from_snapshot(snapshot);
        assert_eq!(restored.id(), original_id);
        assert_eq!(restored.total_fuel_consumed(), 0);
        assert_eq!(restored.remaining_fuel(), Some(1_000_000));
    }

    #[test]
    fn test_session_save_load() {
        let config =
            AgentConfig::builder().memory_limit(128 * 1024 * 1024).max_tool_calls(50).build();

        let session = AgentSession::new(config);
        let original_id = session.id();

        let json = session.save().unwrap();
        assert!(json.contains(&original_id.to_string()));

        let restored = AgentSession::load(&json).unwrap();
        assert_eq!(restored.id(), original_id);
    }

    #[test]
    fn test_session_load_invalid_json() {
        let result = AgentSession::load("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_session_tool_limit_reached() {
        let config = AgentConfig::builder()
            .max_tool_calls(0) // Immediately at limit
            .build();

        let session = AgentSession::new(config);
        assert!(session.is_tool_limit_reached());
    }

    #[test]
    fn test_session_fuel_budget_none() {
        let mut config = AgentConfig::default();
        config.fuel_budget = None;
        let session = AgentSession::new(config);
        assert_eq!(session.remaining_fuel(), None);
    }

    #[test]
    fn test_session_shared_engine() {
        let engine = Arc::new(WasmEngine::new().expect("engine creation"));
        let config = AgentConfig::default();

        let s1 = AgentSession::with_engine(config.clone(), engine.clone());
        let s2 = AgentSession::with_engine(config, engine);

        // Different sessions, different IDs
        assert_ne!(s1.id(), s2.id());
    }

    #[test]
    fn test_session_register_multiple_tools() {
        let mut session = AgentSession::new(AgentConfig::default());
        session.register_tool(ToolDefinition::code_execute());
        session.register_tool(ToolDefinition::file_read());
        session.register_tool(ToolDefinition::file_write());

        assert_eq!(session.tools().len(), 3);
        assert!(session.tools().has("code_execute"));
        assert!(session.tools().has("file_read"));
        assert!(session.tools().has("file_write"));
    }

    #[test]
    fn test_session_history_initially_empty() {
        let session = AgentSession::new(AgentConfig::default());
        assert!(session.history().is_empty());
    }

    #[test]
    fn test_session_config_accessible() {
        let config = AgentConfig::builder()
            .memory_limit(256 * 1024 * 1024)
            .execution_timeout(Duration::from_secs(60))
            .build();

        let session = AgentSession::new(config);
        assert_eq!(session.config().memory_limit, 256 * 1024 * 1024);
        assert_eq!(session.config().execution_timeout, Duration::from_secs(60));
    }
}
