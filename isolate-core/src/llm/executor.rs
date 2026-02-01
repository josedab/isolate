//! Sandbox executor for LLM function calls.
//!
//! Maps LLM function calls to sandboxed WASM executions. Functions are
//! registered with their WASM module bytes, capability requirements, and
//! timeout settings, then executed on demand inside Isolate sandboxes.

use super::provider::{FunctionCall, FunctionDefinition, FunctionResult};
use super::schema::{ParameterSchema, SchemaError, SchemaValidator};
use crate::agent::ResourceUsageSummary;
use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::engine::WasmEngine;
use crate::error::{Error, Result};
use crate::sandbox::Sandbox;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// A function registered for sandbox execution.
#[derive(Debug, Clone)]
pub struct RegisteredFunction {
    /// The function's schema definition.
    pub definition: FunctionDefinition,
    /// Compiled WASM module bytes that implement this function.
    pub module_bytes: Vec<u8>,
    /// Capabilities required by this function (e.g., `"stdout"`, `"stderr"`).
    pub capabilities: Vec<String>,
    /// Maximum execution time for this function.
    pub timeout: Duration,
}

/// Result of executing a function call in a sandbox.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// The function result to return to the LLM.
    pub function_result: FunctionResult,
    /// Exit code from the sandbox process.
    pub sandbox_exit_code: i32,
    /// Resource usage from the sandbox execution.
    pub resource_usage: ResourceUsageSummary,
}

/// Registry of functions available for LLM function calling.
#[derive(Debug, Clone, Default)]
pub struct FunctionRegistry {
    functions: HashMap<String, RegisteredFunction>,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a function for execution.
    pub fn register(&mut self, func: RegisteredFunction) {
        self.functions.insert(func.definition.name.clone(), func);
    }

    /// Look up a registered function by name.
    pub fn get(&self, name: &str) -> Option<&RegisteredFunction> {
        self.functions.get(name)
    }

    /// List all registered function definitions.
    pub fn list_functions(&self) -> Vec<&FunctionDefinition> {
        self.functions.values().map(|f| &f.definition).collect()
    }

    /// Check if a function is registered.
    pub fn has(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Number of registered functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

/// Executes LLM function calls inside Isolate sandboxes.
pub struct FunctionExecutor {
    /// Function registry.
    registry: FunctionRegistry,
    /// Shared WASM engine for module compilation caching.
    engine: Arc<WasmEngine>,
    /// Schema validator.
    validator: SchemaValidator,
}

impl FunctionExecutor {
    /// Create a new executor with a fresh WASM engine.
    pub fn new() -> Self {
        let engine = Arc::new(WasmEngine::new().expect("failed to create WASM engine"));
        Self { registry: FunctionRegistry::new(), engine, validator: SchemaValidator::new() }
    }

    /// Create an executor with a shared WASM engine.
    pub fn with_engine(engine: Arc<WasmEngine>) -> Self {
        Self { registry: FunctionRegistry::new(), engine, validator: SchemaValidator::new() }
    }

    /// Get a reference to the function registry.
    pub fn registry(&self) -> &FunctionRegistry {
        &self.registry
    }

    /// Get a mutable reference to the function registry.
    pub fn registry_mut(&mut self) -> &mut FunctionRegistry {
        &mut self.registry
    }

    /// List all registered function definitions.
    pub fn list_functions(&self) -> Vec<&FunctionDefinition> {
        self.registry.list_functions()
    }

    /// Validate a function call's arguments against the registered schema.
    pub fn validate_call(&self, call: &FunctionCall) -> std::result::Result<(), Vec<SchemaError>> {
        let func = self.registry.get(&call.name).ok_or_else(|| {
            vec![SchemaError::InvalidValue {
                path: "name".to_string(),
                reason: format!("unknown function: {}", call.name),
            }]
        })?;

        let schema = ParameterSchema::from_json_schema(func.definition.parameters.clone());
        self.validator.validate(&schema, &call.arguments)
    }

    /// Execute a function call in a sandbox.
    ///
    /// Looks up the function, optionally validates arguments, builds a
    /// sandbox configuration, runs the WASM module, and returns the result.
    pub async fn execute(&self, call: &FunctionCall) -> Result<ExecutionResult> {
        let func = self
            .registry
            .get(&call.name)
            .ok_or_else(|| Error::FunctionNotFound(call.name.clone()))?;

        // Validate arguments if strict mode is enabled
        if func.definition.strict {
            let schema = ParameterSchema::from_json_schema(func.definition.parameters.clone());
            if let Err(errors) = self.validator.validate(&schema, &call.arguments) {
                let msg = errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
                return Err(Error::Execution(format!(
                    "Schema validation failed for '{}': {}",
                    call.name, msg
                )));
            }
        }

        // Build sandbox config
        let input_json = serde_json::to_vec(&call.arguments)
            .map_err(|e| Error::Execution(format!("Failed to serialize arguments: {}", e)))?;

        let mut builder = SandboxConfig::builder()
            .module(&func.module_bytes)?
            .wall_time_limit(func.timeout)
            .fuel(10_000_000);

        // Apply capabilities
        for cap in &func.capabilities {
            match cap.as_str() {
                "stdout" => builder = builder.capability(Capability::stdout()),
                "stderr" => builder = builder.capability(Capability::stderr()),
                "stdin" => builder = builder.capability(Capability::stdin()),
                _ => {} // Ignore unknown capabilities for forward compatibility
            }
        }

        let config = builder.build()?;
        let start = std::time::Instant::now();

        // Execute in sandbox
        let mut sandbox = Sandbox::create_with_engine(config, self.engine.clone()).await?;
        let output = sandbox.run(&input_json).await;

        let elapsed = start.elapsed();

        match output {
            Ok(output) => {
                let stdout = output.stdout_str();
                let parsed_output = serde_json::from_str::<serde_json::Value>(&stdout)
                    .unwrap_or_else(|_| serde_json::Value::String(stdout));

                let error = if output.exit_code != 0 {
                    Some(format!("exit code: {}", output.exit_code))
                } else {
                    None
                };

                Ok(ExecutionResult {
                    function_result: FunctionResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        output: parsed_output,
                        error,
                        execution_time: elapsed,
                    },
                    sandbox_exit_code: output.exit_code,
                    resource_usage: output.resource_usage.into(),
                })
            }
            Err(e) => Ok(ExecutionResult {
                function_result: FunctionResult {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    output: serde_json::Value::Null,
                    error: Some(e.to_string()),
                    execution_time: elapsed,
                },
                sandbox_exit_code: -1,
                resource_usage: ResourceUsageSummary::default(),
            }),
        }
    }

    /// Parse capability strings into `Capability` values.
    fn _parse_capability(cap: &str) -> Option<Capability> {
        match cap {
            "stdout" => Some(Capability::stdout()),
            "stderr" => Some(Capability::stderr()),
            "stdin" => Some(Capability::stdin()),
            _ => None,
        }
    }
}

impl std::fmt::Debug for FunctionExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionExecutor")
            .field("registry", &self.registry)
            .field("validator", &self.validator)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_registered_function(name: &str, strict: bool) -> RegisteredFunction {
        RegisteredFunction {
            definition: FunctionDefinition {
                name: name.to_string(),
                description: format!("Test function {}", name),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    },
                    "required": ["input"]
                }),
                strict,
            },
            module_bytes: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            capabilities: vec!["stdout".to_string()],
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn test_function_registry_register_and_lookup() {
        let mut registry = FunctionRegistry::new();
        assert!(registry.is_empty());

        registry.register(make_registered_function("test_fn", false));
        assert_eq!(registry.len(), 1);
        assert!(registry.has("test_fn"));
        assert!(!registry.has("nonexistent"));

        let func = registry.get("test_fn").unwrap();
        assert_eq!(func.definition.name, "test_fn");
    }

    #[test]
    fn test_function_registry_list() {
        let mut registry = FunctionRegistry::new();
        registry.register(make_registered_function("fn_a", false));
        registry.register(make_registered_function("fn_b", false));

        let functions = registry.list_functions();
        assert_eq!(functions.len(), 2);

        let names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"fn_a"));
        assert!(names.contains(&"fn_b"));
    }

    #[test]
    fn test_executor_validate_call_valid() {
        let mut executor = FunctionExecutor::new();
        executor.registry_mut().register(make_registered_function("my_func", false));

        let call = FunctionCall {
            id: "call_1".to_string(),
            name: "my_func".to_string(),
            arguments: json!({"input": "hello"}),
        };

        assert!(executor.validate_call(&call).is_ok());
    }

    #[test]
    fn test_executor_validate_call_missing_required() {
        let mut executor = FunctionExecutor::new();
        executor.registry_mut().register(make_registered_function("my_func", false));

        let call = FunctionCall {
            id: "call_2".to_string(),
            name: "my_func".to_string(),
            arguments: json!({}),
        };

        let errs = executor.validate_call(&call).unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            SchemaError::MissingRequired { field } if field == "input"
        )));
    }

    #[test]
    fn test_executor_validate_call_unknown_function() {
        let executor = FunctionExecutor::new();

        let call = FunctionCall {
            id: "call_3".to_string(),
            name: "unknown".to_string(),
            arguments: json!({}),
        };

        let errs = executor.validate_call(&call).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, SchemaError::InvalidValue { .. })));
    }

    #[test]
    fn test_executor_list_functions() {
        let mut executor = FunctionExecutor::new();
        executor.registry_mut().register(make_registered_function("fn_x", false));
        executor.registry_mut().register(make_registered_function("fn_y", true));

        let functions = executor.list_functions();
        assert_eq!(functions.len(), 2);
    }

    #[test]
    fn test_registered_function_clone() {
        let func = make_registered_function("clone_test", true);
        let cloned = func.clone();
        assert_eq!(cloned.definition.name, "clone_test");
        assert!(cloned.definition.strict);
        assert_eq!(cloned.capabilities, vec!["stdout".to_string()]);
        assert_eq!(cloned.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_execution_result_debug() {
        let result = ExecutionResult {
            function_result: FunctionResult {
                call_id: "c1".to_string(),
                name: "fn".to_string(),
                output: json!(42),
                error: None,
                execution_time: Duration::from_millis(50),
            },
            sandbox_exit_code: 0,
            resource_usage: ResourceUsageSummary::default(),
        };
        // Ensure Debug is implemented
        let debug = format!("{:?}", result);
        assert!(debug.contains("ExecutionResult"));
    }
}
