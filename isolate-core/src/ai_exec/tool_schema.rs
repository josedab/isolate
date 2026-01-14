//! OpenAI/Anthropic-compatible tool schemas for AI agent integration.
//!
//! Provides JSON Schema definitions that can be used directly as function/tool
//! definitions in LLM APIs (OpenAI function calling, Anthropic tool use).
//!
//! ```rust
//! use isolate_core::ai_exec::tool_schema::{ToolDefinition, generate_execute_tool};
//!
//! let tool = generate_execute_tool();
//! let json = serde_json::to_string_pretty(&tool).unwrap();
//! assert!(json.contains("execute_code"));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A tool/function definition compatible with OpenAI and Anthropic APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name (must be a valid identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the input parameters.
    pub parameters: SchemaObject,
}

/// A JSON Schema object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaObject {
    /// Schema type.
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Object properties (for type: "object").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, SchemaProperty>>,
    /// Required property names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

/// A property within a JSON Schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaProperty {
    /// Property type.
    #[serde(rename = "type")]
    pub prop_type: String,
    /// Description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Enum values (for constrained strings).
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Items schema (for type: "array").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<SchemaProperty>>,
}

/// Generate the `execute_code` tool definition.
pub fn generate_execute_tool() -> ToolDefinition {
    let mut properties = HashMap::new();

    properties.insert(
        "code".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some("The source code to execute in the sandbox.".to_string()),
            enum_values: None,
            default: None,
            items: None,
        },
    );

    properties.insert(
        "language".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some(
                "Programming language. If omitted, auto-detected from code.".to_string(),
            ),
            enum_values: Some(vec![
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "rust".to_string(),
                "c".to_string(),
                "cpp".to_string(),
                "go".to_string(),
                "wasm".to_string(),
            ]),
            default: None,
            items: None,
        },
    );

    properties.insert(
        "input".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some("Input data provided to the program via stdin.".to_string()),
            enum_values: None,
            default: None,
            items: None,
        },
    );

    properties.insert(
        "timeout_seconds".to_string(),
        SchemaProperty {
            prop_type: "integer".to_string(),
            description: Some(
                "Maximum execution time in seconds (default: 30, max: 300).".to_string(),
            ),
            enum_values: None,
            default: Some(serde_json::Value::Number(30.into())),
            items: None,
        },
    );

    properties.insert(
        "memory_mb".to_string(),
        SchemaProperty {
            prop_type: "integer".to_string(),
            description: Some("Maximum memory in megabytes (default: 128, max: 512).".to_string()),
            enum_values: None,
            default: Some(serde_json::Value::Number(128.into())),
            items: None,
        },
    );

    properties.insert(
        "env".to_string(),
        SchemaProperty {
            prop_type: "object".to_string(),
            description: Some("Environment variables as key-value pairs.".to_string()),
            enum_values: None,
            default: None,
            items: None,
        },
    );

    ToolDefinition {
        name: "execute_code".to_string(),
        description: "Execute source code in a secure WebAssembly sandbox. \
            Returns stdout, stderr, exit code, and resource usage. \
            Code runs in isolation with no network or filesystem access by default."
            .to_string(),
        parameters: SchemaObject {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: Some(vec!["code".to_string()]),
        },
    }
}

/// Generate the `execute_code_batch` tool definition.
pub fn generate_batch_tool() -> ToolDefinition {
    let mut item_props = HashMap::new();

    item_props.insert(
        "code".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some("Source code to execute.".to_string()),
            enum_values: None,
            default: None,
            items: None,
        },
    );

    item_props.insert(
        "language".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some("Programming language.".to_string()),
            enum_values: Some(vec![
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
            ]),
            default: None,
            items: None,
        },
    );

    item_props.insert(
        "id".to_string(),
        SchemaProperty {
            prop_type: "string".to_string(),
            description: Some("Unique identifier for this execution in the batch.".to_string()),
            enum_values: None,
            default: None,
            items: None,
        },
    );

    let mut properties = HashMap::new();
    properties.insert(
        "executions".to_string(),
        SchemaProperty {
            prop_type: "array".to_string(),
            description: Some("Array of code execution requests.".to_string()),
            enum_values: None,
            default: None,
            items: Some(Box::new(SchemaProperty {
                prop_type: "object".to_string(),
                description: None,
                enum_values: None,
                default: None,
                items: None,
            })),
        },
    );

    ToolDefinition {
        name: "execute_code_batch".to_string(),
        description:
            "Execute multiple code snippets in parallel, each in its own isolated sandbox. \
            Returns results for all executions."
                .to_string(),
        parameters: SchemaObject {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: Some(vec!["executions".to_string()]),
        },
    }
}

/// Parse a tool call from the LLM into a CodeRequest.
pub fn parse_tool_call(
    arguments: &serde_json::Value,
) -> std::result::Result<super::CodeRequest, String> {
    let code = arguments
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required field: code".to_string())?;

    let language = arguments.get("language").and_then(|v| v.as_str()).and_then(|s| match s {
        "python" => Some(super::Language::Python),
        "javascript" => Some(super::Language::JavaScript),
        "typescript" => Some(super::Language::TypeScript),
        "rust" => Some(super::Language::Rust),
        "c" => Some(super::Language::C),
        "cpp" => Some(super::Language::Cpp),
        "go" => Some(super::Language::Go),
        "wasm" => Some(super::Language::Wasm),
        _ => None,
    });

    let mut request = if let Some(lang) = language {
        super::CodeRequest::new(code, lang)
    } else {
        super::CodeRequest::auto_detect(code)
    };

    if let Some(input) = arguments.get("input").and_then(|v| v.as_str()) {
        request = request.with_input(input);
    }

    if let Some(env) = arguments.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            if let Some(val_str) = value.as_str() {
                request = request.with_env(key, val_str);
            }
        }
    }

    Ok(request)
}

/// Format a CodeResult as a tool response for the LLM.
pub fn format_tool_response(result: &super::CodeResult) -> serde_json::Value {
    serde_json::json!({
        "success": result.success,
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "duration_ms": result.duration.as_millis(),
        "language": result.language.to_string(),
        "cost": {
            "fuel_consumed": result.cost.fuel_consumed,
            "peak_memory_bytes": result.cost.peak_memory_bytes,
            "cost_units": result.cost.cost_units,
        },
        "output_truncated": result.output_truncated,
    })
}

/// All available tool definitions for AI agent integration.
pub fn all_tools() -> Vec<ToolDefinition> {
    vec![generate_execute_tool(), generate_batch_tool()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_execute_tool() {
        let tool = generate_execute_tool();
        assert_eq!(tool.name, "execute_code");
        assert!(tool.description.contains("sandbox"));

        let props = tool.parameters.properties.as_ref().unwrap();
        assert!(props.contains_key("code"));
        assert!(props.contains_key("language"));
        assert!(props.contains_key("timeout_seconds"));
        assert!(props.contains_key("memory_mb"));

        let required = tool.parameters.required.as_ref().unwrap();
        assert!(required.contains(&"code".to_string()));
    }

    #[test]
    fn test_generate_batch_tool() {
        let tool = generate_batch_tool();
        assert_eq!(tool.name, "execute_code_batch");

        let props = tool.parameters.properties.as_ref().unwrap();
        assert!(props.contains_key("executions"));
    }

    #[test]
    fn test_parse_tool_call_basic() {
        let args = serde_json::json!({
            "code": "print('hello')",
            "language": "python"
        });

        let request = parse_tool_call(&args).unwrap();
        assert_eq!(request.source, "print('hello')");
        assert_eq!(request.language, Some(super::super::Language::Python));
    }

    #[test]
    fn test_parse_tool_call_auto_detect() {
        let args = serde_json::json!({
            "code": "console.log('hello')"
        });

        let request = parse_tool_call(&args).unwrap();
        assert_eq!(request.resolved_language(), Some(super::super::Language::JavaScript));
    }

    #[test]
    fn test_parse_tool_call_with_input() {
        let args = serde_json::json!({
            "code": "import sys; print(sys.stdin.read())",
            "language": "python",
            "input": "test data"
        });

        let request = parse_tool_call(&args).unwrap();
        assert_eq!(request.input, Some("test data".to_string()));
    }

    #[test]
    fn test_parse_tool_call_with_env() {
        let args = serde_json::json!({
            "code": "print('hi')",
            "language": "python",
            "env": {"API_KEY": "test123", "MODE": "debug"}
        });

        let request = parse_tool_call(&args).unwrap();
        assert_eq!(request.env.get("API_KEY"), Some(&"test123".to_string()));
        assert_eq!(request.env.get("MODE"), Some(&"debug".to_string()));
    }

    #[test]
    fn test_parse_tool_call_missing_code() {
        let args = serde_json::json!({"language": "python"});
        assert!(parse_tool_call(&args).is_err());
    }

    #[test]
    fn test_format_tool_response() {
        let result = super::super::CodeResult {
            success: true,
            exit_code: 0,
            stdout: "Hello!".to_string(),
            stderr: String::new(),
            duration: std::time::Duration::from_millis(42),
            language: super::super::Language::Python,
            cost: super::super::CostEstimate {
                fuel_consumed: 1000,
                peak_memory_bytes: 4096,
                io_bytes: 6,
                wall_time_ms: 42.0,
                cost_units: 0.001,
            },
            safety_checks: vec![],
            output_truncated: false,
            output_sanitized: false,
        };

        let response = format_tool_response(&result);
        assert_eq!(response["success"], true);
        assert_eq!(response["exit_code"], 0);
        assert_eq!(response["stdout"], "Hello!");
        assert_eq!(response["cost"]["fuel_consumed"], 1000);
    }

    #[test]
    fn test_tool_serialization() {
        let tool = generate_execute_tool();
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("execute_code"));
        assert!(json.contains("code"));

        // Roundtrip
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "execute_code");
    }

    #[test]
    fn test_all_tools() {
        let tools = all_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "execute_code");
        assert_eq!(tools[1].name, "execute_code_batch");
    }
}
