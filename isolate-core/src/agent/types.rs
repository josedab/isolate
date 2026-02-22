//! Core types for the AI Agent SDK.

use crate::resource::ResourceUsage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for an agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum heap memory per execution (bytes).
    pub memory_limit: usize,
    /// Maximum execution wall time.
    pub execution_timeout: Duration,
    /// Maximum output size in bytes.
    pub max_output_size: usize,
    /// Maximum number of tool calls per session.
    pub max_tool_calls: usize,
    /// Maximum total fuel budget for the session.
    pub fuel_budget: Option<u64>,
    /// Whether to capture stderr separately.
    pub capture_stderr: bool,
    /// Environment variables passed to all executions.
    pub env: HashMap<String, String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            memory_limit: 128 * 1024 * 1024, // 128 MB
            execution_timeout: Duration::from_secs(30),
            max_output_size: 1024 * 1024, // 1 MB
            max_tool_calls: 100,
            fuel_budget: Some(100_000_000),
            capture_stderr: true,
            env: HashMap::new(),
        }
    }
}

impl AgentConfig {
    /// Create a new builder.
    pub fn builder() -> AgentConfigBuilder {
        AgentConfigBuilder::default()
    }
}

/// Builder for [`AgentConfig`].
#[derive(Debug, Default)]
#[must_use = "builders do nothing unless you call .build()"]
pub struct AgentConfigBuilder {
    config: AgentConfig,
}

impl AgentConfigBuilder {
    /// Set the memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.config.memory_limit = bytes;
        self
    }

    /// Set the execution timeout.
    pub fn execution_timeout(mut self, timeout: Duration) -> Self {
        self.config.execution_timeout = timeout;
        self
    }

    /// Set the maximum output size in bytes.
    pub fn max_output_size(mut self, bytes: usize) -> Self {
        self.config.max_output_size = bytes;
        self
    }

    /// Set the maximum number of tool calls per session.
    pub fn max_tool_calls(mut self, limit: usize) -> Self {
        self.config.max_tool_calls = limit;
        self
    }

    /// Set the total fuel budget for the session.
    pub fn fuel_budget(mut self, fuel: u64) -> Self {
        self.config.fuel_budget = Some(fuel);
        self
    }

    /// Set whether to capture stderr.
    pub fn capture_stderr(mut self, capture: bool) -> Self {
        self.config.capture_stderr = capture;
        self
    }

    /// Add an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.env.insert(key.into(), value.into());
        self
    }

    /// Build the configuration.
    pub fn build(self) -> AgentConfig {
        self.config
    }
}

/// A request to execute code in an agent sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExecutionRequest {
    /// WASM module bytes.
    #[serde(skip)]
    pub module_bytes: Vec<u8>,
    /// Structured input data (passed as JSON via stdin).
    pub input: serde_json::Value,
    /// Tool name being invoked (if this is a tool call).
    pub tool_name: Option<String>,
    /// Additional metadata for the execution.
    pub metadata: HashMap<String, String>,
}

impl CodeExecutionRequest {
    /// Create a new code execution request.
    pub fn new(module_bytes: Vec<u8>, input: serde_json::Value) -> Self {
        Self { module_bytes, input, tool_name: None, metadata: HashMap::new() }
    }

    /// Create a tool call request.
    pub fn tool_call(
        module_bytes: Vec<u8>,
        tool_name: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self { module_bytes, input, tool_name: Some(tool_name.into()), metadata: HashMap::new() }
    }

    /// Add metadata to the request.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Result of an agent code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExecutionResult {
    /// Execution status.
    pub status: ExecutionStatus,
    /// Parsed output (from stdout, interpreted as JSON if possible).
    pub output: serde_json::Value,
    /// Raw stdout bytes.
    pub stdout: String,
    /// Raw stderr bytes.
    pub stderr: String,
    /// Exit code from the sandbox.
    pub exit_code: i32,
    /// Execution duration.
    pub duration: Duration,
    /// Resource usage from the execution.
    pub resource_usage: ResourceUsageSummary,
    /// Tool name if this was a tool call.
    pub tool_name: Option<String>,
}

impl CodeExecutionResult {
    /// Check if the execution was successful.
    pub fn success(&self) -> bool {
        matches!(self.status, ExecutionStatus::Success)
    }

    /// Get the output as a typed value.
    pub fn output_as<T: serde::de::DeserializeOwned>(
        &self,
    ) -> std::result::Result<T, serde_json::Error> {
        serde_json::from_value(self.output.clone())
    }
}

/// Status of an agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Execution completed successfully.
    Success,
    /// Execution failed with an error.
    Failed,
    /// Execution timed out.
    Timeout,
    /// Resource limit exceeded (fuel, memory, I/O).
    ResourceExceeded,
    /// Output was truncated due to size limits.
    OutputTruncated,
}

/// Simplified resource usage for agent responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsageSummary {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: usize,
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total bytes written.
    pub bytes_written: u64,
}

impl From<ResourceUsage> for ResourceUsageSummary {
    fn from(usage: ResourceUsage) -> Self {
        Self {
            peak_memory_bytes: usage.peak_memory,
            fuel_consumed: usage.fuel_consumed,
            bytes_read: usage.bytes_read,
            bytes_written: usage.bytes_written,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_builder() {
        let config = AgentConfig::builder()
            .memory_limit(64 * 1024 * 1024)
            .execution_timeout(Duration::from_secs(10))
            .max_output_size(512 * 1024)
            .max_tool_calls(50)
            .fuel_budget(50_000_000)
            .env("API_KEY", "test")
            .build();

        assert_eq!(config.memory_limit, 64 * 1024 * 1024);
        assert_eq!(config.execution_timeout, Duration::from_secs(10));
        assert_eq!(config.max_output_size, 512 * 1024);
        assert_eq!(config.max_tool_calls, 50);
        assert_eq!(config.fuel_budget, Some(50_000_000));
        assert_eq!(config.env.get("API_KEY"), Some(&"test".to_string()));
    }

    #[test]
    fn test_code_execution_request() {
        let request = CodeExecutionRequest::new(
            vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            serde_json::json!({"key": "value"}),
        );

        assert!(request.tool_name.is_none());
        assert_eq!(request.module_bytes.len(), 8);
    }

    #[test]
    fn test_tool_call_request() {
        let request = CodeExecutionRequest::tool_call(
            vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            "code_execute",
            serde_json::json!({"code": "print('hello')"}),
        );

        assert_eq!(request.tool_name, Some("code_execute".to_string()));
    }

    #[test]
    fn test_execution_status() {
        let result = CodeExecutionResult {
            status: ExecutionStatus::Success,
            output: serde_json::json!({"result": 42}),
            stdout: "42".to_string(),
            stderr: String::new(),
            exit_code: 0,
            duration: Duration::from_millis(100),
            resource_usage: ResourceUsageSummary::default(),
            tool_name: None,
        };

        assert!(result.success());
        let val: HashMap<String, i32> = result.output_as().unwrap();
        assert_eq!(val.get("result"), Some(&42));
    }
}
