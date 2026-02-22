//! Tool definitions and registry for AI agent workflows.

use super::protocol::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Definition of a tool that can be called by an AI agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Input parameter schema.
    pub parameters: Vec<ToolParameter>,
    /// Maximum execution timeout in seconds for this tool.
    pub timeout_secs: Option<u64>,
    /// Maximum memory in bytes for this tool.
    pub memory_limit: Option<usize>,
    /// JSON Schema for validating input (optional, auto-generated if not set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<JsonSchema>,
}

impl ToolDefinition {
    /// Create a new tool definition.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: Vec::new(),
            timeout_secs: None,
            memory_limit: None,
            input_schema: None,
        }
    }

    /// Add a parameter to the tool definition.
    pub fn with_parameter(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    /// Set execution timeout for this tool.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Set memory limit for this tool.
    pub fn with_memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = Some(bytes);
        self
    }

    /// Set a JSON Schema for input validation.
    pub fn with_input_schema(mut self, schema: JsonSchema) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Generate a JSON Schema from the parameter list.
    pub fn generate_schema(&self) -> JsonSchema {
        if let Some(ref schema) = self.input_schema {
            return schema.clone();
        }
        let mut builder = JsonSchema::object()
            .description(self.description.clone());
        for param in &self.parameters {
            let param_schema = match param.param_type {
                ToolParameterType::String => JsonSchema::string(),
                ToolParameterType::Integer => JsonSchema::integer(),
                ToolParameterType::Number => JsonSchema::number(),
                ToolParameterType::Boolean => JsonSchema::boolean(),
                ToolParameterType::Object => JsonSchema::object().build(),
                ToolParameterType::Array => JsonSchema::array(JsonSchema::string()),
            };
            if param.required {
                builder = builder.required_property(&param.name, param_schema);
            } else {
                builder = builder.property(&param.name, param_schema);
            }
        }
        builder.build()
    }

    /// Validate input JSON against this tool's schema.
    pub fn validate_input(&self, input: &serde_json::Value) -> Vec<super::protocol::ValidationError> {
        self.generate_schema().validate(input)
    }

    /// Pre-built tool definition for code execution.
    pub fn code_execute() -> Self {
        Self::new("code_execute", "Execute code in a sandboxed WASM environment")
            .with_parameter(ToolParameter {
                name: "code".to_string(),
                description: "The source code or compiled WASM module to execute".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_parameter(ToolParameter {
                name: "input".to_string(),
                description: "Input data to pass via stdin".to_string(),
                param_type: ToolParameterType::Object,
                required: false,
            })
            .with_timeout(30)
    }

    /// Pre-built tool definition for file reading.
    pub fn file_read() -> Self {
        Self::new("file_read", "Read a file from the sandbox filesystem")
            .with_parameter(ToolParameter {
                name: "path".to_string(),
                description: "Path to the file to read".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_timeout(5)
    }

    /// Pre-built tool definition for file writing.
    pub fn file_write() -> Self {
        Self::new("file_write", "Write content to a file in the sandbox filesystem")
            .with_parameter(ToolParameter {
                name: "path".to_string(),
                description: "Path to the file to write".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_parameter(ToolParameter {
                name: "content".to_string(),
                description: "Content to write to the file".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_timeout(5)
    }

    /// Pre-built tool definition for HTTP requests.
    pub fn http_request() -> Self {
        Self::new("http_request", "Make an HTTP request from the sandbox")
            .with_parameter(ToolParameter {
                name: "url".to_string(),
                description: "The URL to request".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_parameter(ToolParameter {
                name: "method".to_string(),
                description: "HTTP method (GET, POST, etc.)".to_string(),
                param_type: ToolParameterType::String,
                required: false,
            })
            .with_parameter(ToolParameter {
                name: "body".to_string(),
                description: "Request body".to_string(),
                param_type: ToolParameterType::String,
                required: false,
            })
            .with_timeout(30)
    }
}

/// A tool parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    /// Parameter name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Parameter type.
    pub param_type: ToolParameterType,
    /// Whether the parameter is required.
    pub required: bool,
}

/// Type of a tool parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolParameterType {
    /// String value.
    String,
    /// Integer value.
    Integer,
    /// Floating point number.
    Number,
    /// Boolean value.
    Boolean,
    /// JSON object.
    Object,
    /// JSON array.
    Array,
}

/// Registry of available tools for an agent session.
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with default tools.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(ToolDefinition::code_execute());
        registry.register(ToolDefinition::file_read());
        registry.register(ToolDefinition::file_write());
        registry.register(ToolDefinition::http_request());
        registry
    }

    /// Register a tool.
    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// Check if a tool is registered.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// List all registered tools.
    pub fn list(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// Get the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Export tool definitions as JSON (for passing to LLM APIs).
    pub fn to_json(&self) -> serde_json::Value {
        let tools: Vec<_> = self.tools.values().collect();
        serde_json::to_value(tools).unwrap_or(serde_json::Value::Array(vec![]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ToolDefinition::new("test_tool", "A test tool")
            .with_parameter(ToolParameter {
                name: "input".to_string(),
                description: "Input data".to_string(),
                param_type: ToolParameterType::String,
                required: true,
            })
            .with_timeout(10)
            .with_memory_limit(64 * 1024 * 1024);

        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.parameters.len(), 1);
        assert_eq!(tool.timeout_secs, Some(10));
        assert_eq!(tool.memory_limit, Some(64 * 1024 * 1024));
    }

    #[test]
    fn test_builtin_tools() {
        let code = ToolDefinition::code_execute();
        assert_eq!(code.name, "code_execute");
        assert_eq!(code.parameters.len(), 2);

        let read = ToolDefinition::file_read();
        assert_eq!(read.name, "file_read");

        let write = ToolDefinition::file_write();
        assert_eq!(write.name, "file_write");

        let http = ToolDefinition::http_request();
        assert_eq!(http.name, "http_request");
    }

    #[test]
    fn test_tool_registry() {
        let mut registry = ToolRegistry::new();
        assert!(registry.is_empty());

        registry.register(ToolDefinition::code_execute());
        assert_eq!(registry.len(), 1);
        assert!(registry.has("code_execute"));
        assert!(!registry.has("nonexistent"));

        let tool = registry.get("code_execute").unwrap();
        assert_eq!(tool.name, "code_execute");
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = ToolRegistry::with_defaults();
        assert_eq!(registry.len(), 4);
        assert!(registry.has("code_execute"));
        assert!(registry.has("file_read"));
        assert!(registry.has("file_write"));
        assert!(registry.has("http_request"));
    }

    #[test]
    fn test_registry_to_json() {
        let registry = ToolRegistry::with_defaults();
        let json = registry.to_json();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 4);
    }

    #[test]
    fn test_tool_generate_schema() {
        let tool = ToolDefinition::code_execute();
        let schema = tool.generate_schema();
        assert_eq!(schema.schema_type, super::super::protocol::JsonSchemaType::Object);
        assert!(schema.required.contains(&"code".to_string()));
        assert!(!schema.required.contains(&"input".to_string()));
    }

    #[test]
    fn test_tool_validate_input_valid() {
        let tool = ToolDefinition::code_execute();
        let input = serde_json::json!({"code": "print('hello')"});
        assert!(tool.validate_input(&input).is_empty());
    }

    #[test]
    fn test_tool_validate_input_missing_required() {
        let tool = ToolDefinition::code_execute();
        let input = serde_json::json!({"input": {}});
        let errors = tool.validate_input(&input);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_tool_validate_input_wrong_type() {
        let tool = ToolDefinition::file_read();
        let input = serde_json::json!({"path": 42});
        let errors = tool.validate_input(&input);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_tool_with_custom_schema() {
        use super::super::protocol::JsonSchema;
        let schema = JsonSchema::object()
            .required_property("query", JsonSchema::string())
            .build();
        let tool = ToolDefinition::new("search", "Search the web")
            .with_input_schema(schema);

        let valid = serde_json::json!({"query": "rust wasm"});
        assert!(tool.validate_input(&valid).is_empty());

        let invalid = serde_json::json!({"query": 123});
        assert!(!tool.validate_input(&invalid).is_empty());
    }
}
