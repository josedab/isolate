//! LLM provider adapters and function calling types.
//!
//! Defines the core types used to represent LLM function definitions,
//! function calls, and results. Supports OpenAI, Anthropic, and custom
//! provider configurations.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Supported LLM providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    /// OpenAI (GPT-4o, etc.)
    OpenAi,
    /// Anthropic (Claude, etc.)
    Anthropic,
    /// Custom provider with a user-defined name.
    Custom {
        /// Provider name.
        name: String,
    },
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::OpenAi => write!(f, "openai"),
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::Custom { name } => write!(f, "custom:{}", name),
        }
    }
}

/// Definition of a function that can be called by an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Function name (must match the tool name the LLM will call).
    pub name: String,
    /// Human-readable description of what the function does.
    pub description: String,
    /// JSON Schema describing the function's parameters.
    pub parameters: serde_json::Value,
    /// Whether to enforce strict schema validation on arguments.
    pub strict: bool,
}

/// A function call requested by an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Unique identifier for this call (provided by the LLM API).
    pub id: String,
    /// Name of the function to invoke.
    pub name: String,
    /// Arguments to pass to the function, as a JSON value.
    pub arguments: serde_json::Value,
}

/// Result of executing a function call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResult {
    /// The call ID this result corresponds to.
    pub call_id: String,
    /// Name of the function that was called.
    pub name: String,
    /// Output produced by the function.
    pub output: serde_json::Value,
    /// Error message, if the function failed.
    pub error: Option<String>,
    /// Time taken to execute the function.
    pub execution_time: Duration,
}

/// Configuration for an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Which LLM provider to use.
    pub provider: LlmProvider,
    /// Maximum number of tokens in the response.
    pub max_tokens: usize,
    /// Sampling temperature (0.0 = deterministic, 2.0 = very random).
    pub temperature: f64,
    /// How the LLM should handle function calling.
    pub function_call_mode: FunctionCallMode,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::OpenAi,
            max_tokens: 4096,
            temperature: 0.7,
            function_call_mode: FunctionCallMode::Auto,
        }
    }
}

/// Controls how the LLM selects function calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionCallMode {
    /// The LLM decides whether and which function to call.
    Auto,
    /// The LLM must call a function.
    Required,
    /// Function calling is disabled.
    None,
    /// The LLM must call the specified function.
    Specific(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_provider_display() {
        assert_eq!(LlmProvider::OpenAi.to_string(), "openai");
        assert_eq!(LlmProvider::Anthropic.to_string(), "anthropic");
        assert_eq!(LlmProvider::Custom { name: "llama".to_string() }.to_string(), "custom:llama");
    }

    #[test]
    fn test_llm_provider_serde_roundtrip() {
        let providers = vec![
            LlmProvider::OpenAi,
            LlmProvider::Anthropic,
            LlmProvider::Custom { name: "local".to_string() },
        ];
        for provider in providers {
            let json = serde_json::to_string(&provider).unwrap();
            let deserialized: LlmProvider = serde_json::from_str(&json).unwrap();
            assert_eq!(provider, deserialized);
        }
    }

    #[test]
    fn test_function_definition_creation() {
        let def = FunctionDefinition {
            name: "get_weather".to_string(),
            description: "Get current weather for a city".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" },
                    "units": { "type": "string", "enum": ["celsius", "fahrenheit"] }
                },
                "required": ["city"]
            }),
            strict: true,
        };

        assert_eq!(def.name, "get_weather");
        assert!(def.strict);
        let props = def.parameters["properties"].as_object().unwrap();
        assert!(props.contains_key("city"));
        assert!(props.contains_key("units"));
    }

    #[test]
    fn test_function_call_serde() {
        let call = FunctionCall {
            id: "call_abc123".to_string(),
            name: "get_weather".to_string(),
            arguments: serde_json::json!({"city": "London"}),
        };

        let json = serde_json::to_string(&call).unwrap();
        let deserialized: FunctionCall = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "call_abc123");
        assert_eq!(deserialized.name, "get_weather");
        assert_eq!(deserialized.arguments["city"], "London");
    }

    #[test]
    fn test_function_result_with_error() {
        let result = FunctionResult {
            call_id: "call_1".to_string(),
            name: "failing_func".to_string(),
            output: serde_json::Value::Null,
            error: Some("sandbox timeout".to_string()),
            execution_time: Duration::from_millis(5000),
        };

        assert!(result.error.is_some());
        assert_eq!(result.error.as_deref(), Some("sandbox timeout"));
    }

    #[test]
    fn test_provider_config_default() {
        let config = ProviderConfig::default();
        assert_eq!(config.provider, LlmProvider::OpenAi);
        assert_eq!(config.max_tokens, 4096);
        assert!((config.temperature - 0.7).abs() < f64::EPSILON);
        assert_eq!(config.function_call_mode, FunctionCallMode::Auto);
    }

    #[test]
    fn test_function_call_mode_serde() {
        let modes = vec![
            FunctionCallMode::Auto,
            FunctionCallMode::Required,
            FunctionCallMode::None,
            FunctionCallMode::Specific("get_weather".to_string()),
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: FunctionCallMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }
}
