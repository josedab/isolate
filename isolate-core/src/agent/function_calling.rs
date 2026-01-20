//! LLM function-calling executor for AI agent integration.
//!
//! Provides an OpenAI-compatible function calling interface that maps
//! tool definitions to JSON Schema and executes tool calls in sandboxes.

use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::engine::WasmEngine;
use crate::error::Result;
use crate::sandbox::Sandbox;

use super::types::ResourceUsageSummary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// An OpenAI-compatible function definition for LLM tool calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Function name (used in tool_call).
    pub name: String,
    /// Human-readable description for the LLM.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// An OpenAI-compatible tool definition wrapping a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Always "function" for function-calling.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: FunctionDefinition,
}

/// A tool call request from an LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool call.
    pub id: String,
    /// Tool type (always "function").
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function call details.
    pub function: FunctionCallInfo,
}

/// Function call details within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallInfo {
    /// Function name to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
}

/// Result of executing a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    /// The tool call ID this result corresponds to.
    pub tool_call_id: String,
    /// The function output as a JSON string.
    pub output: String,
    /// Whether execution succeeded.
    pub success: bool,
    /// Error message if execution failed.
    pub error: Option<String>,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Resource usage summary.
    pub resource_usage: ResourceUsageSummary,
}

/// Configuration for the function call executor.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Default memory limit per execution.
    pub memory_limit: usize,
    /// Default execution timeout.
    pub timeout: Duration,
    /// Default fuel budget per call.
    pub fuel: u64,
    /// Maximum output size in bytes.
    pub max_output_bytes: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            memory_limit: 64 * 1024 * 1024,
            timeout: Duration::from_secs(30),
            fuel: 10_000_000,
            max_output_bytes: 1024 * 1024,
        }
    }
}

/// Executor for LLM function calls using sandboxed WASM modules.
pub struct FunctionCallExecutor {
    engine: Arc<WasmEngine>,
    modules: HashMap<String, Vec<u8>>,
    tool_specs: Vec<ToolSpec>,
    default_config: ExecutorConfig,
}

impl FunctionCallExecutor {
    /// Create a new executor with default configuration.
    pub fn new() -> Result<Self> {
        Self::with_config(ExecutorConfig::default())
    }

    /// Create an executor with custom configuration.
    pub fn with_config(config: ExecutorConfig) -> Result<Self> {
        let engine = Arc::new(WasmEngine::new()?);
        Ok(Self {
            engine,
            modules: HashMap::new(),
            tool_specs: Vec::new(),
            default_config: config,
        })
    }

    /// Create an executor with a shared engine.
    pub fn with_engine(engine: Arc<WasmEngine>, config: ExecutorConfig) -> Self {
        Self { engine, modules: HashMap::new(), tool_specs: Vec::new(), default_config: config }
    }

    /// Register a function backed by a WASM module.
    ///
    /// The WASM module should read JSON from stdin and write JSON to stdout.
    pub fn register_function(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: serde_json::Value,
        wasm_bytes: Vec<u8>,
    ) {
        let name = name.into();
        self.tool_specs.push(ToolSpec {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.clone(),
                description: description.into(),
                parameters: parameters_schema,
            },
        });
        self.modules.insert(name, wasm_bytes);
    }

    /// Get tool specs for sending to an LLM (OpenAI-compatible format).
    pub fn tool_specs(&self) -> &[ToolSpec] {
        &self.tool_specs
    }

    /// Generate the `tools` parameter for an OpenAI API request.
    pub fn tools_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.tool_specs).unwrap_or_default()
    }

    /// Execute a single tool call.
    pub async fn execute_tool_call(&self, tool_call: &ToolCall) -> ToolCallResult {
        let start = Instant::now();

        let wasm_bytes = match self.modules.get(&tool_call.function.name) {
            Some(bytes) => bytes,
            None => {
                return ToolCallResult {
                    tool_call_id: tool_call.id.clone(),
                    output: String::new(),
                    success: false,
                    error: Some(format!("Unknown function: {}", tool_call.function.name)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    resource_usage: ResourceUsageSummary::default(),
                };
            }
        };

        // Validate arguments JSON
        let input: serde_json::Value = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolCallResult {
                    tool_call_id: tool_call.id.clone(),
                    output: String::new(),
                    success: false,
                    error: Some(format!("Invalid arguments JSON: {}", e)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    resource_usage: ResourceUsageSummary::default(),
                };
            }
        };

        // Build sandbox config
        let config = match SandboxConfig::builder()
            .module(wasm_bytes)
            .and_then(|b| {
                Ok(b.memory_limit(self.default_config.memory_limit)
                    .fuel(self.default_config.fuel)
                    .wall_time_limit(self.default_config.timeout)
                    .capability(Capability::stdout())
                    .capability(Capability::stderr())
                    .capability(Capability::stdin())
                    .build()?)
            }) {
            Ok(c) => c,
            Err(e) => {
                return ToolCallResult {
                    tool_call_id: tool_call.id.clone(),
                    output: String::new(),
                    success: false,
                    error: Some(format!("Failed to configure sandbox: {}", e)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    resource_usage: ResourceUsageSummary::default(),
                };
            }
        };

        // Execute
        let input_bytes = serde_json::to_vec(&input).unwrap_or_default();
        match Sandbox::create_with_engine(config, self.engine.clone()).await {
            Ok(mut sandbox) => match sandbox.run(&input_bytes).await {
                Ok(output) => {
                    let stdout = output.stdout_str();
                    let truncated = if stdout.len() > self.default_config.max_output_bytes {
                        stdout[..self.default_config.max_output_bytes].to_string()
                    } else {
                        stdout
                    };

                    ToolCallResult {
                        tool_call_id: tool_call.id.clone(),
                        output: truncated,
                        success: output.exit_code == 0,
                        error: if output.exit_code != 0 {
                            Some(output.stderr_str())
                        } else {
                            None
                        },
                        duration_ms: start.elapsed().as_millis() as u64,
                        resource_usage: output.resource_usage.into(),
                    }
                }
                Err(e) => ToolCallResult {
                    tool_call_id: tool_call.id.clone(),
                    output: String::new(),
                    success: false,
                    error: Some(format!("Execution failed: {}", e)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    resource_usage: ResourceUsageSummary::default(),
                },
            },
            Err(e) => ToolCallResult {
                tool_call_id: tool_call.id.clone(),
                output: String::new(),
                success: false,
                error: Some(format!("Sandbox creation failed: {}", e)),
                duration_ms: start.elapsed().as_millis() as u64,
                resource_usage: ResourceUsageSummary::default(),
            },
        }
    }

    /// Execute multiple tool calls sequentially.
    pub async fn execute_tool_calls(&self, tool_calls: &[ToolCall]) -> Vec<ToolCallResult> {
        let mut results = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            results.push(self.execute_tool_call(call).await);
        }
        results
    }

    /// Get the list of registered function names.
    pub fn function_names(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a function is registered.
    pub fn has_function(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }
}

impl Default for FunctionCallExecutor {
    fn default() -> Self {
        Self::new().expect("Failed to create default function call executor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let executor = FunctionCallExecutor::new().unwrap();
        assert!(executor.tool_specs().is_empty());
        assert!(executor.function_names().is_empty());
    }

    #[test]
    fn test_register_function() {
        let mut executor = FunctionCallExecutor::new().unwrap();

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "x": { "type": "number" },
                "y": { "type": "number" }
            },
            "required": ["x", "y"]
        });

        executor.register_function("add", "Add two numbers", schema, vec![0; 8]);

        assert_eq!(executor.function_names().len(), 1);
        assert!(executor.has_function("add"));
        assert!(!executor.has_function("subtract"));
    }

    #[test]
    fn test_tool_specs_openai_format() {
        let mut executor = FunctionCallExecutor::new().unwrap();

        let schema = serde_json::json!({
            "type": "object",
            "properties": { "code": { "type": "string" } }
        });

        executor.register_function("run_code", "Execute code", schema, vec![0; 8]);

        let specs = executor.tool_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].tool_type, "function");
        assert_eq!(specs[0].function.name, "run_code");
    }

    #[test]
    fn test_tools_json_serialization() {
        let mut executor = FunctionCallExecutor::new().unwrap();

        executor.register_function(
            "test",
            "A test function",
            serde_json::json!({"type": "object"}),
            vec![0; 8],
        );

        let json = executor.tools_json();
        assert!(json.is_array());

        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "test");
    }

    #[tokio::test]
    async fn test_execute_unknown_function() {
        let executor = FunctionCallExecutor::new().unwrap();

        let call = ToolCall {
            id: "call-1".to_string(),
            tool_type: "function".to_string(),
            function: FunctionCallInfo {
                name: "unknown".to_string(),
                arguments: "{}".to_string(),
            },
        };

        let result = executor.execute_tool_call(&call).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown function"));
    }

    #[tokio::test]
    async fn test_execute_invalid_json_arguments() {
        let mut executor = FunctionCallExecutor::new().unwrap();
        executor.register_function("test", "test", serde_json::json!({}), vec![0; 8]);

        let call = ToolCall {
            id: "call-2".to_string(),
            tool_type: "function".to_string(),
            function: FunctionCallInfo {
                name: "test".to_string(),
                arguments: "not json".to_string(),
            },
        };

        let result = executor.execute_tool_call(&call).await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Invalid arguments JSON"));
    }

    #[test]
    fn test_function_definition_serialization() {
        let def = FunctionDefinition {
            name: "get_weather".to_string(),
            description: "Get current weather".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "location": { "type": "string" }
                },
                "required": ["location"]
            }),
        };

        let json = serde_json::to_value(&def).unwrap();
        assert_eq!(json["name"], "get_weather");
        assert!(json["parameters"]["properties"]["location"].is_object());
    }

    #[test]
    fn test_tool_call_result_serialization() {
        let result = ToolCallResult {
            tool_call_id: "call-123".to_string(),
            output: r#"{"temp": 72}"#.to_string(),
            success: true,
            error: None,
            duration_ms: 45,
            resource_usage: ResourceUsageSummary::default(),
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["tool_call_id"], "call-123");
        assert_eq!(json["success"], true);
        assert!(json["error"].is_null());
    }
}
