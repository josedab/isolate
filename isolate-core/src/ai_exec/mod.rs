//! AI Code Execution Sandbox.
//!
//! High-level API for executing AI-generated code in secure sandboxes.
//! Supports language detection, output sanitization, cost estimation,
//! and streaming output.
//!
//! # Features
//!
//! - **Language Detection**: Automatic detection of source code language
//! - **Output Sanitization**: Configurable output filtering and size limits
//! - **Cost Estimation**: Estimate resource cost before execution
//! - **Execution Profiles**: Pre-configured profiles for common AI use cases
//! - **Safety Defaults**: Conservative defaults suitable for untrusted AI-generated code
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::ai_exec::{CodeRequest, CodeExecutor, ExecutionProfile};
//!
//! let executor = CodeExecutor::new(ExecutionProfile::conservative());
//!
//! let request = CodeRequest::new("print('Hello from AI!')", Language::Python);
//! let result = executor.execute(request).await?;
//!
//! println!("Output: {}", result.stdout);
//! println!("Cost: {} fuel units", result.cost.fuel_consumed);
//! ```

// This module is experimental and not all APIs are used yet.


mod executor;
pub mod tool_schema;

pub use executor::{
    CodeExecutor, CodeRequest, CodeResult, CostEstimate, ExecutionProfile, Language,
    OutputSanitizer, SafetyCheck, SafetyLevel, SanitizeConfig,
};
pub use tool_schema::{
    all_tools, format_tool_response, generate_batch_tool, generate_execute_tool, parse_tool_call,
    ToolDefinition,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let profile = ExecutionProfile::conservative();
        assert_eq!(profile.safety_level, SafetyLevel::Strict);
    }
}
