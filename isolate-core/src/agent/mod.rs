//! AI Agent Sandbox SDK.
//!
//! Purpose-built SDK for LLM/AI agent code execution with structured
//! input/output, tool-use integration, and conversation-aware sandboxing.
//!
//! # Overview
//!
//! The agent module provides a higher-level abstraction over raw sandbox
//! execution, designed specifically for AI agent workflows:
//!
//! - **Structured I/O**: JSON-based request/response protocol
//! - **Tool definitions**: Register callable tools with schemas
//! - **Session management**: Stateful sessions with execution history
//! - **Safety guardrails**: Output size limits, execution budgets
//!
//! # Example
//!
//! ```no_run
//! use isolate_core::agent::{AgentSession, AgentConfig, ToolDefinition, CodeExecutionRequest};
//!
//! # async fn example() -> isolate_core::Result<()> {
//! let config = AgentConfig::builder()
//!     .memory_limit(128 * 1024 * 1024)
//!     .execution_timeout(std::time::Duration::from_secs(30))
//!     .max_output_size(1024 * 1024)
//!     .build();
//!
//! let mut session = AgentSession::new(config);
//!
//! // Register a tool
//! session.register_tool(ToolDefinition::code_execute());
//!
//! // Execute code
//! let request = CodeExecutionRequest::new(
//!     std::fs::read("agent_code.wasm")?,
//!     serde_json::json!({"query": "analyze this data"}),
//! );
//!
//! let result = session.execute(request).await?;
//! println!("Output: {}", result.output);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

mod session;
mod tools;
mod types;

pub use session::{AgentSession, ExecutionRecord};
pub use tools::{ToolDefinition, ToolParameter, ToolParameterType, ToolRegistry};
pub use types::{
    AgentConfig, AgentConfigBuilder, CodeExecutionRequest, CodeExecutionResult, ExecutionStatus,
    ResourceUsageSummary,
};
