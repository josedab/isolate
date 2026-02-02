use super::function::{InvocationRequest, InvocationResponse, ServerlessFunction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Runtime handler that bridges HTTP requests to sandbox execution.
pub struct RuntimeHandler {
    functions: HashMap<String, ServerlessFunction>,
    cold_start_count: u64,
    warm_start_count: u64,
    total_invocations: u64,
    error_count: u64,
    total_duration_ms: u64,
    max_duration_ms: u64,
}

impl RuntimeHandler {
    /// Create a new runtime handler.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            cold_start_count: 0,
            warm_start_count: 0,
            total_invocations: 0,
            error_count: 0,
            total_duration_ms: 0,
            max_duration_ms: 0,
        }
    }

    /// Register a serverless function.
    pub fn register(&mut self, function: ServerlessFunction) {
        self.functions.insert(function.name.clone(), function);
    }

    /// Handle an invocation request and produce a response.
    pub fn handle(&mut self, request: InvocationRequest) -> Result<InvocationResponse, String> {
        let function = self
            .functions
            .get(&request.function_name)
            .ok_or_else(|| format!("Function '{}' not found", request.function_name))?;

        let is_cold_start =
            self.total_invocations == 0 || !self.functions.contains_key(&request.function_name);

        if is_cold_start {
            self.cold_start_count += 1;
        } else {
            self.warm_start_count += 1;
        }
        self.total_invocations += 1;

        // Simulate sandbox execution: serialize payload as the sandbox input,
        // produce a response based on function config.
        let duration_ms = 1; // simulated fast execution
        self.total_duration_ms += duration_ms;
        if duration_ms > self.max_duration_ms {
            self.max_duration_ms = duration_ms;
        }

        let mut response_headers = HashMap::new();
        response_headers.insert("content-type".to_string(), "application/json".to_string());
        response_headers.insert("x-function-name".to_string(), function.name.clone());

        Ok(InvocationResponse {
            status_code: 200,
            body: serde_json::json!({
                "result": "ok",
                "function": function.name,
                "payload": request.payload,
            }),
            headers: response_headers,
            duration_ms,
            request_id: request.request_id,
            cold_start: is_cold_start,
        })
    }

    /// Get runtime metrics.
    pub fn get_metrics(&self) -> RuntimeMetrics {
        let avg_duration_ms = if self.total_invocations > 0 {
            self.total_duration_ms as f64 / self.total_invocations as f64
        } else {
            0.0
        };

        RuntimeMetrics {
            total_invocations: self.total_invocations,
            cold_starts: self.cold_start_count,
            warm_starts: self.warm_start_count,
            avg_duration_ms,
            p99_duration_ms: self.max_duration_ms as f64,
            error_count: self.error_count,
            active_instances: self.functions.len() as u32,
        }
    }

    /// List all registered functions.
    pub fn list_functions(&self) -> Vec<&ServerlessFunction> {
        self.functions.values().collect()
    }

    /// Remove a function by name. Returns true if the function was found and removed.
    pub fn remove_function(&mut self, name: &str) -> bool {
        self.functions.remove(name).is_some()
    }
}

impl Default for RuntimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub total_invocations: u64,
    pub cold_starts: u64,
    pub warm_starts: u64,
    pub avg_duration_ms: f64,
    pub p99_duration_ms: f64,
    pub error_count: u64,
    pub active_instances: u32,
}

/// Execution context provided to the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionContext {
    pub function_name: String,
    pub request_id: String,
    pub memory_limit_mb: u64,
    pub timeout_remaining_ms: u64,
    pub cold_start: bool,
    pub invocation_count: u64,
    pub environment: HashMap<String, String>,
}

impl FunctionContext {
    /// Create a context from a function and request.
    pub fn from_function(
        function: &ServerlessFunction,
        request_id: &str,
        cold_start: bool,
        invocation_count: u64,
    ) -> Self {
        Self {
            function_name: function.name.clone(),
            request_id: request_id.to_string(),
            memory_limit_mb: function.runtime.memory_mb,
            timeout_remaining_ms: function.runtime.timeout.as_millis() as u64,
            cold_start,
            invocation_count,
            environment: function.environment.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverless::function::{FunctionBuilder, HttpMethod};

    fn sample_function(name: &str) -> ServerlessFunction {
        FunctionBuilder::new(name)
            .description("test function")
            .module_path("./test.wasm")
            .memory_mb(256)
            .http_trigger("/api/test", vec![HttpMethod::Get])
            .env("KEY", "value")
            .build()
    }

    fn sample_request(function_name: &str) -> InvocationRequest {
        InvocationRequest {
            function_name: function_name.to_string(),
            payload: serde_json::json!({"input": "data"}),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            request_id: "req-001".to_string(),
        }
    }

    #[test]
    fn test_runtime_handler_register_and_list() {
        let mut handler = RuntimeHandler::new();
        handler.register(sample_function("func-a"));
        handler.register(sample_function("func-b"));

        let funcs = handler.list_functions();
        assert_eq!(funcs.len(), 2);
    }

    #[test]
    fn test_runtime_handler_handle_success() {
        let mut handler = RuntimeHandler::new();
        handler.register(sample_function("my-func"));

        let request = sample_request("my-func");
        let response = handler.handle(request).unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.request_id, "req-001");
        assert!(response.body["function"] == "my-func");
    }

    #[test]
    fn test_runtime_handler_handle_not_found() {
        let mut handler = RuntimeHandler::new();
        let request = sample_request("nonexistent");
        let result = handler.handle(request);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_runtime_handler_remove_function() {
        let mut handler = RuntimeHandler::new();
        handler.register(sample_function("func-a"));
        assert_eq!(handler.list_functions().len(), 1);

        assert!(handler.remove_function("func-a"));
        assert_eq!(handler.list_functions().len(), 0);

        assert!(!handler.remove_function("func-a"));
    }

    #[test]
    fn test_runtime_metrics_initial() {
        let handler = RuntimeHandler::new();
        let metrics = handler.get_metrics();

        assert_eq!(metrics.total_invocations, 0);
        assert_eq!(metrics.cold_starts, 0);
        assert_eq!(metrics.warm_starts, 0);
        assert_eq!(metrics.error_count, 0);
        assert_eq!(metrics.active_instances, 0);
    }

    #[test]
    fn test_runtime_metrics_after_invocations() {
        let mut handler = RuntimeHandler::new();
        handler.register(sample_function("func-a"));

        // First invocation is a cold start
        handler.handle(sample_request("func-a")).unwrap();
        let metrics = handler.get_metrics();
        assert_eq!(metrics.total_invocations, 1);
        assert_eq!(metrics.cold_starts, 1);

        // Subsequent invocations are warm
        handler.handle(sample_request("func-a")).unwrap();
        handler.handle(sample_request("func-a")).unwrap();
        let metrics = handler.get_metrics();
        assert_eq!(metrics.total_invocations, 3);
        assert_eq!(metrics.warm_starts, 2);
        assert_eq!(metrics.active_instances, 1);
    }

    #[test]
    fn test_function_context_from_function() {
        let func = sample_function("ctx-func");
        let ctx = FunctionContext::from_function(&func, "req-42", true, 5);

        assert_eq!(ctx.function_name, "ctx-func");
        assert_eq!(ctx.request_id, "req-42");
        assert_eq!(ctx.memory_limit_mb, 256);
        assert!(ctx.cold_start);
        assert_eq!(ctx.invocation_count, 5);
        assert_eq!(ctx.environment["KEY"], "value");
    }

    #[test]
    fn test_function_context_serialization() {
        let func = sample_function("ser-func");
        let ctx = FunctionContext::from_function(&func, "req-99", false, 10);

        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: FunctionContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.function_name, "ser-func");
        assert_eq!(deserialized.request_id, "req-99");
        assert!(!deserialized.cold_start);
        assert_eq!(deserialized.invocation_count, 10);
    }

    #[test]
    fn test_runtime_metrics_serialization() {
        let metrics = RuntimeMetrics {
            total_invocations: 100,
            cold_starts: 5,
            warm_starts: 95,
            avg_duration_ms: 2.5,
            p99_duration_ms: 15.0,
            error_count: 1,
            active_instances: 3,
        };

        let json = serde_json::to_string(&metrics).unwrap();
        let deserialized: RuntimeMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_invocations, 100);
        assert_eq!(deserialized.active_instances, 3);
    }

    #[test]
    fn test_runtime_handler_default() {
        let handler = RuntimeHandler::default();
        assert_eq!(handler.list_functions().len(), 0);
        assert_eq!(handler.get_metrics().total_invocations, 0);
    }
}
