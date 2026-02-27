//! Structured JSON I/O protocol for agent communication.
//!
//! Provides GA-quality message envelopes, schema validation, and budget
//! enforcement for the agent tool-call pipeline.

use super::trace::ResourceBudget;
use super::types::ResourceUsageSummary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// JSON Schema
// ---------------------------------------------------------------------------

/// Lightweight JSON Schema representation for validating tool inputs/outputs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonSchema {
    /// Root type of the schema.
    #[serde(rename = "type")]
    pub schema_type: JsonSchemaType,
    /// Properties (only meaningful for `Object` type).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, JsonSchema>,
    /// Required property names (only meaningful for `Object` type).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    /// Item schema (only meaningful for `Array` type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Supported JSON Schema types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JsonSchemaType {
    Object,
    String,
    Number,
    Integer,
    Boolean,
    Array,
}

impl JsonSchema {
    /// Start building an object schema.
    pub fn object() -> JsonSchemaBuilder {
        JsonSchemaBuilder {
            schema_type: JsonSchemaType::Object,
            properties: HashMap::new(),
            required: Vec::new(),
            items: None,
            description: None,
        }
    }

    /// Create a simple string schema.
    pub fn string() -> Self {
        Self {
            schema_type: JsonSchemaType::String,
            properties: HashMap::new(),
            required: Vec::new(),
            items: None,
            description: None,
        }
    }

    /// Create a simple number schema.
    pub fn number() -> Self {
        Self {
            schema_type: JsonSchemaType::Number,
            properties: HashMap::new(),
            required: Vec::new(),
            items: None,
            description: None,
        }
    }

    /// Create a simple integer schema.
    pub fn integer() -> Self {
        Self {
            schema_type: JsonSchemaType::Integer,
            properties: HashMap::new(),
            required: Vec::new(),
            items: None,
            description: None,
        }
    }

    /// Create a simple boolean schema.
    pub fn boolean() -> Self {
        Self {
            schema_type: JsonSchemaType::Boolean,
            properties: HashMap::new(),
            required: Vec::new(),
            items: None,
            description: None,
        }
    }

    /// Create an array schema with the given item schema.
    pub fn array(items: JsonSchema) -> Self {
        Self {
            schema_type: JsonSchemaType::Array,
            properties: HashMap::new(),
            required: Vec::new(),
            items: Some(Box::new(items)),
            description: None,
        }
    }

    /// Validate a `serde_json::Value` against this schema.
    ///
    /// Returns a list of validation errors (empty on success).
    pub fn validate(&self, value: &serde_json::Value) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        self.validate_inner(value, "$", &mut errors);
        errors
    }

    fn validate_inner(
        &self,
        value: &serde_json::Value,
        path: &str,
        errors: &mut Vec<ValidationError>,
    ) {
        match self.schema_type {
            JsonSchemaType::Object => {
                let Some(obj) = value.as_object() else {
                    errors.push(ValidationError::type_mismatch(path, "object", value));
                    return;
                };
                // Check required fields.
                for key in &self.required {
                    if !obj.contains_key(key) {
                        errors.push(ValidationError::missing_field(path, key));
                    }
                }
                // Validate known properties.
                for (key, schema) in &self.properties {
                    if let Some(v) = obj.get(key) {
                        let child_path = format!("{path}.{key}");
                        schema.validate_inner(v, &child_path, errors);
                    }
                }
            }
            JsonSchemaType::String => {
                if !value.is_string() {
                    errors.push(ValidationError::type_mismatch(path, "string", value));
                }
            }
            JsonSchemaType::Number => {
                if !value.is_number() {
                    errors.push(ValidationError::type_mismatch(path, "number", value));
                }
            }
            JsonSchemaType::Integer => {
                if !value.is_i64() && !value.is_u64() {
                    errors.push(ValidationError::type_mismatch(path, "integer", value));
                }
            }
            JsonSchemaType::Boolean => {
                if !value.is_boolean() {
                    errors.push(ValidationError::type_mismatch(path, "boolean", value));
                }
            }
            JsonSchemaType::Array => {
                let Some(arr) = value.as_array() else {
                    errors.push(ValidationError::type_mismatch(path, "array", value));
                    return;
                };
                if let Some(item_schema) = &self.items {
                    for (i, item) in arr.iter().enumerate() {
                        let child_path = format!("{path}[{i}]");
                        item_schema.validate_inner(item, &child_path, errors);
                    }
                }
            }
        }
    }
}

/// Builder for constructing [`JsonSchema`] objects.
#[derive(Debug)]
pub struct JsonSchemaBuilder {
    schema_type: JsonSchemaType,
    properties: HashMap<String, JsonSchema>,
    required: Vec<String>,
    items: Option<Box<JsonSchema>>,
    description: Option<String>,
}

impl JsonSchemaBuilder {
    /// Add an optional property.
    pub fn property(mut self, name: impl Into<String>, schema: JsonSchema) -> Self {
        self.properties.insert(name.into(), schema);
        self
    }

    /// Add a required property.
    pub fn required_property(mut self, name: impl Into<String>, schema: JsonSchema) -> Self {
        let name = name.into();
        self.required.push(name.clone());
        self.properties.insert(name, schema);
        self
    }

    /// Set a human-readable description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Build the schema.
    pub fn build(self) -> JsonSchema {
        JsonSchema {
            schema_type: self.schema_type,
            properties: self.properties,
            required: self.required,
            items: self.items,
            description: self.description,
        }
    }
}

// ---------------------------------------------------------------------------
// Validation error
// ---------------------------------------------------------------------------

/// A single schema-validation error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    /// JSON-pointer-style path to the offending value.
    pub path: String,
    /// Human-readable message.
    pub message: String,
}

impl ValidationError {
    fn type_mismatch(path: &str, expected: &str, actual: &serde_json::Value) -> Self {
        let actual_type = match actual {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        };
        Self { path: path.to_string(), message: format!("expected {expected}, got {actual_type}") }
    }

    fn missing_field(path: &str, field: &str) -> Self {
        Self { path: path.to_string(), message: format!("missing required field \"{field}\"") }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

// ---------------------------------------------------------------------------
// Protocol messages
// ---------------------------------------------------------------------------

/// Structured protocol envelope for agent communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    /// A request to invoke a tool.
    AgentRequest {
        /// Name of the tool to invoke.
        tool_name: String,
        /// Structured input for the tool.
        input: serde_json::Value,
        /// Caller-assigned call identifier.
        call_id: Uuid,
        /// Per-call resource budget.
        budget: ResourceBudget,
    },
    /// A successful tool response.
    AgentResponse {
        /// Structured output from the tool.
        output: serde_json::Value,
        /// Execution status (e.g. "success", "failed").
        status: String,
        /// Trace identifier for correlation.
        trace_id: Uuid,
        /// Resource usage incurred by the call.
        resource_usage: ResourceUsageSummary,
    },
    /// An error response.
    ErrorResponse {
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
        /// Optional structured details.
        details: Option<serde_json::Value>,
    },
}

impl ProtocolMessage {
    /// Serialize this message to a single JSON line (newline-terminated).
    pub fn to_json_line(&self) -> serde_json::Result<String> {
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }

    /// Deserialize a protocol message from a JSON line.
    pub fn from_json_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line.trim())
    }

    /// Parse multiple newline-delimited messages.
    pub fn from_json_lines(text: &str) -> Vec<serde_json::Result<Self>> {
        text.lines().filter(|l| !l.trim().is_empty()).map(Self::from_json_line).collect()
    }
}

// ---------------------------------------------------------------------------
// Protocol validator
// ---------------------------------------------------------------------------

/// Validates request/response messages against registered schemas.
#[derive(Debug, Default)]
pub struct ProtocolValidator {
    input_schemas: HashMap<String, JsonSchema>,
    output_schemas: HashMap<String, JsonSchema>,
}

impl ProtocolValidator {
    /// Create an empty validator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an input schema for a tool.
    pub fn register_input_schema(&mut self, tool_name: impl Into<String>, schema: JsonSchema) {
        self.input_schemas.insert(tool_name.into(), schema);
    }

    /// Register an output schema for a tool.
    pub fn register_output_schema(&mut self, tool_name: impl Into<String>, schema: JsonSchema) {
        self.output_schemas.insert(tool_name.into(), schema);
    }

    /// Validate an [`AgentRequest`](ProtocolMessage::AgentRequest) message.
    ///
    /// Returns validation errors; empty means valid.
    pub fn validate_request(&self, msg: &ProtocolMessage) -> Vec<ValidationError> {
        match msg {
            ProtocolMessage::AgentRequest { tool_name, input, .. } => {
                if let Some(schema) = self.input_schemas.get(tool_name) {
                    schema.validate(input)
                } else {
                    Vec::new() // no schema registered → pass
                }
            }
            _ => vec![ValidationError {
                path: "$".to_string(),
                message: "expected AgentRequest message".to_string(),
            }],
        }
    }

    /// Validate an [`AgentResponse`](ProtocolMessage::AgentResponse) against the
    /// output schema of the given tool.
    pub fn validate_response(
        &self,
        tool_name: &str,
        msg: &ProtocolMessage,
    ) -> Vec<ValidationError> {
        match msg {
            ProtocolMessage::AgentResponse { output, .. } => {
                if let Some(schema) = self.output_schemas.get(tool_name) {
                    schema.validate(output)
                } else {
                    Vec::new()
                }
            }
            _ => vec![ValidationError {
                path: "$".to_string(),
                message: "expected AgentResponse message".to_string(),
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Budget enforcer
// ---------------------------------------------------------------------------

/// A single budget-limit violation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetViolation {
    /// Which resource was violated (e.g. "fuel", "memory").
    pub resource: String,
    /// The limit that was set.
    pub limit: u64,
    /// The actual (or requested) value.
    pub actual: u64,
    /// Human-readable message.
    pub message: String,
}

impl std::fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Enforces per-call resource budgets at GA quality.
#[derive(Debug)]
pub struct BudgetEnforcer {
    budget: ResourceBudget,
}

impl BudgetEnforcer {
    /// Create an enforcer for the given budget.
    pub fn new(budget: ResourceBudget) -> Self {
        Self { budget }
    }

    /// Check fuel budget. Returns a violation if `fuel` exceeds the limit.
    pub fn check_fuel(&self, fuel: u64) -> Option<BudgetViolation> {
        self.budget.max_fuel.and_then(|limit| {
            (fuel > limit).then(|| BudgetViolation {
                resource: "fuel".to_string(),
                limit,
                actual: fuel,
                message: format!("fuel {fuel} exceeds limit {limit}"),
            })
        })
    }

    /// Check memory budget.
    pub fn check_memory(&self, bytes: usize) -> Option<BudgetViolation> {
        self.budget.max_memory_bytes.and_then(|limit| {
            (bytes > limit).then(|| BudgetViolation {
                resource: "memory".to_string(),
                limit: limit as u64,
                actual: bytes as u64,
                message: format!("memory {bytes} bytes exceeds limit {limit} bytes"),
            })
        })
    }

    /// Check wall-time budget.
    pub fn check_wall_time(&self, elapsed: Duration) -> Option<BudgetViolation> {
        self.budget.max_wall_time.and_then(|limit| {
            (elapsed > limit).then(|| BudgetViolation {
                resource: "wall_time".to_string(),
                limit: limit.as_millis() as u64,
                actual: elapsed.as_millis() as u64,
                message: format!(
                    "wall time {}ms exceeds limit {}ms",
                    elapsed.as_millis(),
                    limit.as_millis()
                ),
            })
        })
    }

    /// Check output-bytes budget.
    pub fn check_output_bytes(&self, bytes: usize) -> Option<BudgetViolation> {
        self.budget.max_output_bytes.and_then(|limit| {
            (bytes > limit).then(|| BudgetViolation {
                resource: "output_bytes".to_string(),
                limit: limit as u64,
                actual: bytes as u64,
                message: format!("output {bytes} bytes exceeds limit {limit} bytes"),
            })
        })
    }

    /// Check I/O operations budget.
    pub fn check_io_ops(&self, ops: u64) -> Option<BudgetViolation> {
        self.budget.max_io_ops.and_then(|limit| {
            (ops > limit).then(|| BudgetViolation {
                resource: "io_ops".to_string(),
                limit,
                actual: ops,
                message: format!("I/O ops {ops} exceeds limit {limit}"),
            })
        })
    }

    /// Run all checks against a [`ResourceUsageSummary`] and an optional
    /// elapsed duration, returning every violation found.
    pub fn check_all(
        &self,
        usage: &ResourceUsageSummary,
        elapsed: Option<Duration>,
    ) -> Vec<BudgetViolation> {
        let mut violations = Vec::new();
        if let Some(v) = self.check_fuel(usage.fuel_consumed) {
            violations.push(v);
        }
        if let Some(v) = self.check_memory(usage.peak_memory_bytes) {
            violations.push(v);
        }
        if let Some(elapsed) = elapsed {
            if let Some(v) = self.check_wall_time(elapsed) {
                violations.push(v);
            }
        }
        let total_io = usage.bytes_read.saturating_add(usage.bytes_written);
        if let Some(v) = self.check_output_bytes(total_io as usize) {
            violations.push(v);
        }
        violations
    }
}

// ---------------------------------------------------------------------------
// Protocol Adapters (LangChain, OpenAI, Anthropic)
// ---------------------------------------------------------------------------

/// Protocol adapter format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFormat {
    /// Native Isolate JSON-line protocol.
    Native,
    /// OpenAI function calling format.
    OpenAi,
    /// Anthropic tool-use format.
    Anthropic,
    /// LangChain tool format.
    LangChain,
}

/// Converts between Isolate's native protocol and external LLM provider formats.
#[derive(Debug, Clone)]
pub struct ProtocolAdapter {
    format: ProtocolFormat,
}

impl ProtocolAdapter {
    /// Create a new adapter for the given format.
    pub fn new(format: ProtocolFormat) -> Self {
        Self { format }
    }

    /// Convert tool definitions to the target provider format.
    pub fn export_tools(&self, tools: &[super::tools::ToolDefinition]) -> serde_json::Value {
        match self.format {
            ProtocolFormat::Native => serde_json::to_value(tools).unwrap_or_default(),
            ProtocolFormat::OpenAi => self.to_openai_functions(tools),
            ProtocolFormat::Anthropic => self.to_anthropic_tools(tools),
            ProtocolFormat::LangChain => self.to_langchain_tools(tools),
        }
    }

    /// Parse a tool call from the provider format into a ProtocolMessage.
    pub fn parse_tool_call(&self, value: &serde_json::Value) -> Result<ProtocolMessage, String> {
        match self.format {
            ProtocolFormat::Native => serde_json::from_value(value.clone())
                .map_err(|e| format!("Invalid native message: {}", e)),
            ProtocolFormat::OpenAi => self.parse_openai_call(value),
            ProtocolFormat::Anthropic => self.parse_anthropic_call(value),
            ProtocolFormat::LangChain => self.parse_langchain_call(value),
        }
    }

    /// Format a response for the target provider.
    pub fn format_response(&self, msg: &ProtocolMessage) -> serde_json::Value {
        match self.format {
            ProtocolFormat::Native => serde_json::to_value(msg).unwrap_or_default(),
            ProtocolFormat::OpenAi => self.to_openai_response(msg),
            ProtocolFormat::Anthropic => self.to_anthropic_response(msg),
            ProtocolFormat::LangChain => self.to_langchain_response(msg),
        }
    }

    fn to_openai_functions(&self, tools: &[super::tools::ToolDefinition]) -> serde_json::Value {
        let functions: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let schema = t.generate_schema();
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": schema,
                    }
                })
            })
            .collect();
        serde_json::Value::Array(functions)
    }

    fn to_anthropic_tools(&self, tools: &[super::tools::ToolDefinition]) -> serde_json::Value {
        let items: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let schema = t.generate_schema();
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": schema,
                })
            })
            .collect();
        serde_json::Value::Array(items)
    }

    fn to_langchain_tools(&self, tools: &[super::tools::ToolDefinition]) -> serde_json::Value {
        let items: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let schema = t.generate_schema();
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "args_schema": schema,
                    "return_direct": false,
                })
            })
            .collect();
        serde_json::Value::Array(items)
    }

    fn parse_openai_call(&self, value: &serde_json::Value) -> Result<ProtocolMessage, String> {
        let name = value
            .get("function")
            .and_then(|f| f.get("name"))
            .or_else(|| value.get("name"))
            .and_then(|v| v.as_str())
            .ok_or("Missing function name in OpenAI call")?;
        let arguments = value
            .get("function")
            .and_then(|f| f.get("arguments"))
            .or_else(|| value.get("arguments"))
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let input = if let Some(s) = arguments.as_str() {
            serde_json::from_str(s).unwrap_or(serde_json::Value::String(s.to_string()))
        } else {
            arguments
        };
        let call_id_str = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let call_id = Uuid::parse_str(call_id_str).unwrap_or_else(|_| Uuid::new_v4());
        Ok(ProtocolMessage::AgentRequest {
            tool_name: name.to_string(),
            input,
            call_id,
            budget: ResourceBudget::default(),
        })
    }

    fn parse_anthropic_call(&self, value: &serde_json::Value) -> Result<ProtocolMessage, String> {
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing tool name in Anthropic call")?;
        let input =
            value.get("input").cloned().unwrap_or(serde_json::Value::Object(Default::default()));
        let id_str = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let call_id = Uuid::parse_str(id_str).unwrap_or_else(|_| Uuid::new_v4());
        Ok(ProtocolMessage::AgentRequest {
            tool_name: name.to_string(),
            input,
            call_id,
            budget: ResourceBudget::default(),
        })
    }

    fn parse_langchain_call(&self, value: &serde_json::Value) -> Result<ProtocolMessage, String> {
        let name = value
            .get("tool")
            .or_else(|| value.get("name"))
            .and_then(|v| v.as_str())
            .ok_or("Missing tool name in LangChain call")?;
        let input = value
            .get("tool_input")
            .or_else(|| value.get("args"))
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        Ok(ProtocolMessage::AgentRequest {
            tool_name: name.to_string(),
            input,
            call_id: Uuid::new_v4(),
            budget: ResourceBudget::default(),
        })
    }

    fn to_openai_response(&self, msg: &ProtocolMessage) -> serde_json::Value {
        match msg {
            ProtocolMessage::AgentResponse { output, .. } => {
                serde_json::json!({
                    "role": "tool",
                    "content": serde_json::to_string(output).unwrap_or_default(),
                })
            }
            ProtocolMessage::ErrorResponse { code, message, .. } => {
                serde_json::json!({
                    "role": "tool",
                    "content": format!("Error [{}]: {}", code, message),
                })
            }
            _ => serde_json::to_value(msg).unwrap_or_default(),
        }
    }

    fn to_anthropic_response(&self, msg: &ProtocolMessage) -> serde_json::Value {
        match msg {
            ProtocolMessage::AgentResponse { output, .. } => {
                serde_json::json!({
                    "type": "tool_result",
                    "content": output,
                })
            }
            ProtocolMessage::ErrorResponse { code, message, .. } => {
                serde_json::json!({
                    "type": "tool_result",
                    "is_error": true,
                    "content": format!("[{}] {}", code, message),
                })
            }
            _ => serde_json::to_value(msg).unwrap_or_default(),
        }
    }

    fn to_langchain_response(&self, msg: &ProtocolMessage) -> serde_json::Value {
        match msg {
            ProtocolMessage::AgentResponse { output, status, .. } => {
                serde_json::json!({
                    "output": output,
                    "status": status,
                })
            }
            ProtocolMessage::ErrorResponse { code, message, .. } => {
                serde_json::json!({
                    "output": format!("[{}] {}", code, message),
                    "status": "error",
                })
            }
            _ => serde_json::to_value(msg).unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- JsonSchema tests ---------------------------------------------------

    #[test]
    fn test_schema_validate_string() {
        let schema = JsonSchema::string();
        assert!(schema.validate(&serde_json::json!("hello")).is_empty());
        assert!(!schema.validate(&serde_json::json!(42)).is_empty());
    }

    #[test]
    fn test_schema_validate_number() {
        let schema = JsonSchema::number();
        assert!(schema.validate(&serde_json::json!(3.14)).is_empty());
        assert!(schema.validate(&serde_json::json!(42)).is_empty());
        assert!(!schema.validate(&serde_json::json!("text")).is_empty());
    }

    #[test]
    fn test_schema_validate_integer() {
        let schema = JsonSchema::integer();
        assert!(schema.validate(&serde_json::json!(42)).is_empty());
        assert!(!schema.validate(&serde_json::json!(3.14)).is_empty());
        assert!(!schema.validate(&serde_json::json!("text")).is_empty());
    }

    #[test]
    fn test_schema_validate_boolean() {
        let schema = JsonSchema::boolean();
        assert!(schema.validate(&serde_json::json!(true)).is_empty());
        assert!(!schema.validate(&serde_json::json!(1)).is_empty());
    }

    #[test]
    fn test_schema_validate_array() {
        let schema = JsonSchema::array(JsonSchema::integer());
        assert!(schema.validate(&serde_json::json!([1, 2, 3])).is_empty());

        let errs = schema.validate(&serde_json::json!([1, "two", 3]));
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.contains("[1]"));
    }

    #[test]
    fn test_schema_validate_object_required() {
        let schema = JsonSchema::object()
            .required_property("name", JsonSchema::string())
            .required_property("age", JsonSchema::integer())
            .property("email", JsonSchema::string())
            .build();

        // Valid.
        let valid = serde_json::json!({"name": "Alice", "age": 30});
        assert!(schema.validate(&valid).is_empty());

        // Missing required field.
        let missing = serde_json::json!({"name": "Alice"});
        let errs = schema.validate(&missing);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("age"));

        // Wrong type.
        let wrong = serde_json::json!({"name": "Alice", "age": "thirty"});
        let errs = schema.validate(&wrong);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.contains("age"));
    }

    #[test]
    fn test_schema_validate_nested_object() {
        let inner = JsonSchema::object().required_property("street", JsonSchema::string()).build();
        let schema = JsonSchema::object().required_property("address", inner).build();

        let valid = serde_json::json!({"address": {"street": "123 Main"}});
        assert!(schema.validate(&valid).is_empty());

        let bad = serde_json::json!({"address": {"street": 123}});
        let errs = schema.validate(&bad);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].path.contains("street"));
    }

    #[test]
    fn test_schema_builder_description() {
        let schema = JsonSchema::object()
            .description("A person record")
            .required_property("name", JsonSchema::string())
            .build();
        assert_eq!(schema.description.as_deref(), Some("A person record"));
    }

    #[test]
    fn test_schema_serialization_roundtrip() {
        let schema = JsonSchema::object().required_property("query", JsonSchema::string()).build();
        let json = serde_json::to_string(&schema).unwrap();
        let deser: JsonSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, deser);
    }

    // -- ProtocolMessage tests ----------------------------------------------

    #[test]
    fn test_agent_request_json_line_roundtrip() {
        let msg = ProtocolMessage::AgentRequest {
            tool_name: "code_execute".into(),
            input: serde_json::json!({"code": "1+1"}),
            call_id: Uuid::nil(),
            budget: ResourceBudget::default(),
        };

        let line = msg.to_json_line().unwrap();
        assert!(line.ends_with('\n'));

        let parsed = ProtocolMessage::from_json_line(&line).unwrap();
        let ProtocolMessage::AgentRequest { tool_name, .. } = parsed else {
            unreachable!("expected AgentRequest");
        };
        assert_eq!(tool_name, "code_execute");
    }

    #[test]
    fn test_agent_response_json_line_roundtrip() {
        let msg = ProtocolMessage::AgentResponse {
            output: serde_json::json!({"result": 2}),
            status: "success".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };

        let line = msg.to_json_line().unwrap();
        let parsed = ProtocolMessage::from_json_line(&line).unwrap();
        let ProtocolMessage::AgentResponse { status, .. } = parsed else {
            unreachable!("expected AgentResponse");
        };
        assert_eq!(status, "success");
    }

    #[test]
    fn test_error_response_json_line() {
        let msg = ProtocolMessage::ErrorResponse {
            code: "INVALID_INPUT".into(),
            message: "missing field".into(),
            details: Some(serde_json::json!({"field": "name"})),
        };

        let line = msg.to_json_line().unwrap();
        let parsed = ProtocolMessage::from_json_line(&line).unwrap();
        let ProtocolMessage::ErrorResponse { code, .. } = parsed else {
            unreachable!("expected ErrorResponse");
        };
        assert_eq!(code, "INVALID_INPUT");
    }

    #[test]
    fn test_from_json_lines() {
        let req = ProtocolMessage::AgentRequest {
            tool_name: "a".into(),
            input: serde_json::json!(null),
            call_id: Uuid::nil(),
            budget: ResourceBudget::default(),
        };
        let resp = ProtocolMessage::AgentResponse {
            output: serde_json::json!(null),
            status: "ok".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };

        let mut text = req.to_json_line().unwrap();
        text.push_str(&resp.to_json_line().unwrap());

        let msgs = ProtocolMessage::from_json_lines(&text);
        assert_eq!(msgs.len(), 2);
        assert!(msgs.iter().all(|r| r.is_ok()));
    }

    // -- ProtocolValidator tests --------------------------------------------

    #[test]
    fn test_validator_accepts_valid_request() {
        let mut v = ProtocolValidator::new();
        v.register_input_schema(
            "tool_a",
            JsonSchema::object().required_property("query", JsonSchema::string()).build(),
        );

        let msg = ProtocolMessage::AgentRequest {
            tool_name: "tool_a".into(),
            input: serde_json::json!({"query": "hi"}),
            call_id: Uuid::nil(),
            budget: ResourceBudget::default(),
        };

        assert!(v.validate_request(&msg).is_empty());
    }

    #[test]
    fn test_validator_rejects_invalid_request() {
        let mut v = ProtocolValidator::new();
        v.register_input_schema(
            "tool_a",
            JsonSchema::object().required_property("query", JsonSchema::string()).build(),
        );

        let msg = ProtocolMessage::AgentRequest {
            tool_name: "tool_a".into(),
            input: serde_json::json!({"query": 42}),
            call_id: Uuid::nil(),
            budget: ResourceBudget::default(),
        };

        let errs = v.validate_request(&msg);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn test_validator_passes_unknown_tool() {
        let v = ProtocolValidator::new();
        let msg = ProtocolMessage::AgentRequest {
            tool_name: "unknown".into(),
            input: serde_json::json!({}),
            call_id: Uuid::nil(),
            budget: ResourceBudget::default(),
        };
        assert!(v.validate_request(&msg).is_empty());
    }

    #[test]
    fn test_validator_rejects_wrong_message_type() {
        let v = ProtocolValidator::new();
        let msg =
            ProtocolMessage::ErrorResponse { code: "E".into(), message: "m".into(), details: None };
        assert!(!v.validate_request(&msg).is_empty());
    }

    #[test]
    fn test_validator_response_validation() {
        let mut v = ProtocolValidator::new();
        v.register_output_schema(
            "tool_a",
            JsonSchema::object().required_property("result", JsonSchema::integer()).build(),
        );

        let good = ProtocolMessage::AgentResponse {
            output: serde_json::json!({"result": 42}),
            status: "success".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };
        assert!(v.validate_response("tool_a", &good).is_empty());

        let bad = ProtocolMessage::AgentResponse {
            output: serde_json::json!({"result": "not_a_number"}),
            status: "success".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };
        assert!(!v.validate_response("tool_a", &bad).is_empty());
    }

    // -- BudgetEnforcer tests -----------------------------------------------

    #[test]
    fn test_budget_enforcer_fuel() {
        let enforcer = BudgetEnforcer::new(ResourceBudget::default().with_fuel(1000));
        assert!(enforcer.check_fuel(500).is_none());
        let v = enforcer.check_fuel(2000).unwrap();
        assert_eq!(v.resource, "fuel");
        assert_eq!(v.limit, 1000);
        assert_eq!(v.actual, 2000);
    }

    #[test]
    fn test_budget_enforcer_memory() {
        let enforcer = BudgetEnforcer::new(ResourceBudget::default().with_memory(1024));
        assert!(enforcer.check_memory(512).is_none());
        assert!(enforcer.check_memory(2048).is_some());
    }

    #[test]
    fn test_budget_enforcer_wall_time() {
        let budget = ResourceBudget::default().with_wall_time(Duration::from_secs(5));
        let enforcer = BudgetEnforcer::new(budget);
        assert!(enforcer.check_wall_time(Duration::from_secs(3)).is_none());
        let v = enforcer.check_wall_time(Duration::from_secs(10)).unwrap();
        assert_eq!(v.resource, "wall_time");
    }

    #[test]
    fn test_budget_enforcer_output_bytes() {
        let budget = ResourceBudget { max_output_bytes: Some(100), ..ResourceBudget::default() };
        let enforcer = BudgetEnforcer::new(budget);
        assert!(enforcer.check_output_bytes(50).is_none());
        assert!(enforcer.check_output_bytes(200).is_some());
    }

    #[test]
    fn test_budget_enforcer_io_ops() {
        let budget = ResourceBudget { max_io_ops: Some(10), ..ResourceBudget::default() };
        let enforcer = BudgetEnforcer::new(budget);
        assert!(enforcer.check_io_ops(5).is_none());
        assert!(enforcer.check_io_ops(20).is_some());
    }

    #[test]
    fn test_budget_enforcer_check_all_within() {
        let enforcer = BudgetEnforcer::new(ResourceBudget::default());
        let usage = ResourceUsageSummary {
            fuel_consumed: 100,
            peak_memory_bytes: 1024,
            bytes_read: 0,
            bytes_written: 0,
        };
        assert!(enforcer.check_all(&usage, Some(Duration::from_secs(1))).is_empty());
    }

    #[test]
    fn test_budget_enforcer_check_all_multiple_violations() {
        let budget = ResourceBudget {
            max_fuel: Some(100),
            max_memory_bytes: Some(512),
            max_wall_time: Some(Duration::from_secs(1)),
            max_output_bytes: Some(10),
            max_io_ops: None,
        };
        let enforcer = BudgetEnforcer::new(budget);
        let usage = ResourceUsageSummary {
            fuel_consumed: 200,
            peak_memory_bytes: 1024,
            bytes_read: 50,
            bytes_written: 50,
        };

        let violations = enforcer.check_all(&usage, Some(Duration::from_secs(5)));
        assert_eq!(violations.len(), 4); // fuel + memory + wall_time + output_bytes
    }

    #[test]
    fn test_budget_enforcer_no_limits() {
        let budget = ResourceBudget {
            max_fuel: None,
            max_memory_bytes: None,
            max_wall_time: None,
            max_output_bytes: None,
            max_io_ops: None,
        };
        let enforcer = BudgetEnforcer::new(budget);
        let usage = ResourceUsageSummary {
            fuel_consumed: u64::MAX,
            peak_memory_bytes: usize::MAX,
            bytes_read: u64::MAX,
            bytes_written: u64::MAX,
        };
        assert!(enforcer.check_all(&usage, Some(Duration::from_secs(9999))).is_empty());
    }

    #[test]
    fn test_budget_violation_display() {
        let v = BudgetViolation {
            resource: "fuel".into(),
            limit: 100,
            actual: 200,
            message: "fuel 200 exceeds limit 100".into(),
        };
        assert_eq!(v.to_string(), "fuel 200 exceeds limit 100");
    }

    #[test]
    fn test_validation_error_display() {
        let e = ValidationError {
            path: "$.name".into(),
            message: "expected string, got number".into(),
        };
        assert_eq!(e.to_string(), "$.name: expected string, got number");
    }

    // -- ProtocolAdapter tests ----------------------------------------------

    #[test]
    fn test_adapter_openai_export() {
        use super::super::tools::ToolDefinition;
        let adapter = ProtocolAdapter::new(ProtocolFormat::OpenAi);
        let tools = vec![ToolDefinition::code_execute()];
        let exported = adapter.export_tools(&tools);
        let arr = exported.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "code_execute");
    }

    #[test]
    fn test_adapter_anthropic_export() {
        use super::super::tools::ToolDefinition;
        let adapter = ProtocolAdapter::new(ProtocolFormat::Anthropic);
        let tools = vec![ToolDefinition::file_read()];
        let exported = adapter.export_tools(&tools);
        let arr = exported.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "file_read");
        assert!(arr[0].get("input_schema").is_some());
    }

    #[test]
    fn test_adapter_langchain_export() {
        use super::super::tools::ToolDefinition;
        let adapter = ProtocolAdapter::new(ProtocolFormat::LangChain);
        let tools = vec![ToolDefinition::http_request()];
        let exported = adapter.export_tools(&tools);
        let arr = exported.as_array().unwrap();
        assert_eq!(arr[0]["name"], "http_request");
        assert_eq!(arr[0]["return_direct"], false);
    }

    #[test]
    fn test_adapter_openai_parse_call() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::OpenAi);
        let call = serde_json::json!({
            "function": {
                "name": "code_execute",
                "arguments": "{\"code\": \"1+1\"}"
            }
        });
        let msg = adapter.parse_tool_call(&call).unwrap();
        match msg {
            ProtocolMessage::AgentRequest { tool_name, input, .. } => {
                assert_eq!(tool_name, "code_execute");
                assert_eq!(input["code"], "1+1");
            }
            _ => panic!("Expected AgentRequest"),
        }
    }

    #[test]
    fn test_adapter_anthropic_parse_call() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::Anthropic);
        let call = serde_json::json!({
            "name": "file_read",
            "input": {"path": "/tmp/test.txt"}
        });
        let msg = adapter.parse_tool_call(&call).unwrap();
        match msg {
            ProtocolMessage::AgentRequest { tool_name, input, .. } => {
                assert_eq!(tool_name, "file_read");
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            _ => panic!("Expected AgentRequest"),
        }
    }

    #[test]
    fn test_adapter_langchain_parse_call() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::LangChain);
        let call = serde_json::json!({
            "tool": "http_request",
            "tool_input": {"url": "https://example.com"}
        });
        let msg = adapter.parse_tool_call(&call).unwrap();
        match msg {
            ProtocolMessage::AgentRequest { tool_name, input, .. } => {
                assert_eq!(tool_name, "http_request");
                assert_eq!(input["url"], "https://example.com");
            }
            _ => panic!("Expected AgentRequest"),
        }
    }

    #[test]
    fn test_adapter_format_response_openai() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::OpenAi);
        let msg = ProtocolMessage::AgentResponse {
            output: serde_json::json!({"result": 42}),
            status: "success".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };
        let resp = adapter.format_response(&msg);
        assert_eq!(resp["role"], "tool");
    }

    #[test]
    fn test_adapter_format_response_anthropic() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::Anthropic);
        let msg = ProtocolMessage::AgentResponse {
            output: serde_json::json!({"data": "hello"}),
            status: "success".into(),
            trace_id: Uuid::nil(),
            resource_usage: ResourceUsageSummary::default(),
        };
        let resp = adapter.format_response(&msg);
        assert_eq!(resp["type"], "tool_result");
    }

    #[test]
    fn test_adapter_format_error_anthropic() {
        let adapter = ProtocolAdapter::new(ProtocolFormat::Anthropic);
        let msg = ProtocolMessage::ErrorResponse {
            code: "TIMEOUT".into(),
            message: "execution timed out".into(),
            details: None,
        };
        let resp = adapter.format_response(&msg);
        assert_eq!(resp["is_error"], true);
    }
}
