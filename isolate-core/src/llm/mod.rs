//! LLM Function Calling Toolkit.
//!
//! Provides integration between LLM function calling APIs and Isolate's
//! sandboxed WASM execution. This module enables LLM agents to define
//! functions backed by WASM modules, validate call arguments against
//! JSON Schema, track token usage and costs, and execute function calls
//! in isolated sandboxes.
//!
//! # Overview
//!
//! - **Provider adapters**: Abstractions for OpenAI, Anthropic, and custom LLM providers
//! - **Schema validation**: Validate function call arguments against JSON Schema
//! - **Token tracking**: Monitor token usage and estimate costs across sessions
//! - **Sandbox executor**: Map LLM function calls to sandboxed WASM executions
//!
//! # Example
//!
//! ```no_run
//! use isolate_core::llm::{
//!     FunctionDefinition, FunctionCall, FunctionRegistry, FunctionExecutor,
//!     RegisteredFunction, TokenTracker, TokenBudget, LlmProvider,
//! };
//! use std::time::Duration;
//!
//! # async fn example() -> isolate_core::Result<()> {
//! // Define a function backed by a WASM module
//! let func = RegisteredFunction {
//!     definition: FunctionDefinition {
//!         name: "calculate".to_string(),
//!         description: "Perform a calculation".to_string(),
//!         parameters: serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "expression": { "type": "string" }
//!             },
//!             "required": ["expression"]
//!         }),
//!         strict: false,
//!     },
//!     module_bytes: std::fs::read("calc.wasm")?,
//!     capabilities: vec!["stdout".to_string()],
//!     timeout: Duration::from_secs(10),
//! };
//!
//! // Register and execute
//! let mut executor = FunctionExecutor::new();
//! executor.registry_mut().register(func);
//!
//! let call = FunctionCall {
//!     id: "call_1".to_string(),
//!     name: "calculate".to_string(),
//!     arguments: serde_json::json!({"expression": "2+2"}),
//! };
//!
//! let result = executor.execute(&call).await?;
//! println!("Result: {}", result.function_result.output);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

mod executor;
mod provider;
mod schema;
mod token;

pub use executor::{ExecutionResult, FunctionExecutor, FunctionRegistry, RegisteredFunction};
pub use provider::{
    FunctionCall, FunctionCallMode, FunctionDefinition, FunctionResult, LlmProvider, ProviderConfig,
};
pub use schema::{ParameterSchema, SchemaError, SchemaValidator};
pub use token::{CostEstimate, PricingTier, TokenBudget, TokenTracker, TokenUsage};
