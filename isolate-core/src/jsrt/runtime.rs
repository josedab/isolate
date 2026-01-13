//! JavaScript runtime implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Configuration for the JavaScript runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsRuntimeConfig {
    /// Maximum execution time per script.
    pub max_execution_time: Duration,
    /// Maximum memory for the JS engine.
    pub max_memory: usize,
    /// Maximum output size.
    pub max_output_bytes: usize,
    /// Enable console.log/warn/error bridging.
    pub enable_console: bool,
    /// Enable setTimeout/setInterval (limited).
    pub enable_timers: bool,
    /// Enable TextEncoder/TextDecoder.
    pub enable_text_codec: bool,
    /// Enable TypeScript transpilation.
    pub enable_typescript: bool,
    /// TypeScript transpilation config.
    pub transpile_config: TranspileConfig,
    /// Custom host bindings.
    pub host_bindings: Vec<HostBinding>,
    /// Maximum script source size.
    pub max_source_bytes: usize,
}

impl Default for JsRuntimeConfig {
    fn default() -> Self {
        Self {
            max_execution_time: Duration::from_secs(30),
            max_memory: 128 * 1024 * 1024, // 128 MB
            max_output_bytes: 1024 * 1024, // 1 MB
            enable_console: true,
            enable_timers: false,
            enable_text_codec: true,
            enable_typescript: false,
            transpile_config: TranspileConfig::default(),
            host_bindings: Vec::new(),
            max_source_bytes: 5 * 1024 * 1024, // 5 MB
        }
    }
}

/// TypeScript transpilation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranspileConfig {
    /// Target ECMAScript version.
    pub target: EsTarget,
    /// Enable JSX transformation.
    pub jsx: bool,
    /// Enable source maps.
    pub source_maps: bool,
}

impl Default for TranspileConfig {
    fn default() -> Self {
        Self { target: EsTarget::Es2022, jsx: false, source_maps: false }
    }
}

/// ECMAScript target version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EsTarget {
    Es2020,
    Es2021,
    Es2022,
    Es2023,
    EsNext,
}

impl std::fmt::Display for EsTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Es2020 => write!(f, "ES2020"),
            Self::Es2021 => write!(f, "ES2021"),
            Self::Es2022 => write!(f, "ES2022"),
            Self::Es2023 => write!(f, "ES2023"),
            Self::EsNext => write!(f, "ESNext"),
        }
    }
}

/// A custom host binding exposed to JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostBinding {
    /// JavaScript function name.
    pub name: String,
    /// Namespace (e.g., "isolate.fs", "isolate.http").
    pub namespace: String,
    /// Binding type.
    pub binding_type: HostBindingType,
    /// Description.
    pub description: String,
}

/// Type of host binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostBindingType {
    /// Synchronous function.
    Sync { params: Vec<String>, return_type: String },
    /// Asynchronous function.
    Async { params: Vec<String>, return_type: String },
    /// Constant value.
    Constant { value_type: String },
}

/// A request to execute JavaScript code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsRequest {
    /// JavaScript or TypeScript source code.
    pub source: String,
    /// Whether the source is TypeScript.
    pub is_typescript: bool,
    /// Input data (accessible as `Isolate.input`).
    pub input: Option<String>,
    /// Environment variables (accessible as `Isolate.env`).
    pub env: HashMap<String, String>,
    /// Import map for module resolution.
    pub import_map: HashMap<String, String>,
    /// Request metadata.
    pub metadata: HashMap<String, String>,
}

impl JsRequest {
    /// Create a new JavaScript request.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            is_typescript: false,
            input: None,
            env: HashMap::new(),
            import_map: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a TypeScript request.
    pub fn typescript(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            is_typescript: true,
            input: None,
            env: HashMap::new(),
            import_map: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set input data.
    pub fn with_input(mut self, input: impl Into<String>) -> Self {
        self.input = Some(input.into());
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Add an import mapping.
    pub fn with_import(mut self, specifier: impl Into<String>, url: impl Into<String>) -> Self {
        self.import_map.insert(specifier.into(), url.into());
        self
    }
}

/// Result of JavaScript execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsResult {
    /// Whether execution succeeded.
    pub success: bool,
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Stdout output (from console.log).
    pub stdout: String,
    /// Stderr output (from console.error/warn).
    pub stderr: String,
    /// Return value (JSON-serialized).
    pub return_value: Option<String>,
    /// Execution duration.
    pub duration: Duration,
    /// Memory used.
    pub memory_bytes: u64,
    /// Whether TypeScript transpilation was performed.
    pub transpiled: bool,
    /// Error message (if failed).
    pub error: Option<JsError>,
}

/// JavaScript error details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsError {
    /// Error name (e.g., "TypeError", "ReferenceError").
    pub name: String,
    /// Error message.
    pub message: String,
    /// Stack trace.
    pub stack: Option<String>,
    /// Line number.
    pub line: Option<u32>,
    /// Column number.
    pub column: Option<u32>,
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.message)?;
        if let Some(line) = self.line {
            write!(f, " at line {}", line)?;
        }
        Ok(())
    }
}

/// Validation result for a JS request.
#[derive(Debug, Clone)]
pub struct JsValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl JsValidation {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Wrapper script that bridges Isolate host functions to JavaScript globals.
pub fn generate_wrapper(request: &JsRequest, config: &JsRuntimeConfig) -> String {
    let mut wrapper = String::new();

    // Console polyfill
    if config.enable_console {
        wrapper.push_str("const __stdout = [];\n");
        wrapper.push_str("const __stderr = [];\n");
        wrapper.push_str("const console = {\n");
        wrapper.push_str("  log: (...args) => __stdout.push(args.map(String).join(' ')),\n");
        wrapper.push_str("  error: (...args) => __stderr.push(args.map(String).join(' ')),\n");
        wrapper.push_str(
            "  warn: (...args) => __stderr.push('WARN: ' + args.map(String).join(' ')),\n",
        );
        wrapper.push_str("  info: (...args) => __stdout.push(args.map(String).join(' ')),\n");
        wrapper.push_str("};\n");
    }

    // Isolate global
    wrapper.push_str("const Isolate = {\n");
    if let Some(ref input) = request.input {
        wrapper.push_str(&format!(
            "  input: {},\n",
            serde_json::to_string(input).unwrap_or_else(|_| "null".to_string())
        ));
    } else {
        wrapper.push_str("  input: null,\n");
    }

    // Environment variables
    wrapper.push_str("  env: {\n");
    for (key, value) in &request.env {
        wrapper.push_str(&format!(
            "    {}: {},\n",
            key,
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
        ));
    }
    wrapper.push_str("  },\n");
    wrapper.push_str("};\n\n");

    // User code
    wrapper.push_str("// === User Code ===\n");
    wrapper.push_str(&request.source);
    wrapper.push_str("\n// === End User Code ===\n");

    wrapper
}

/// Detect potential issues in JavaScript code.
fn detect_js_issues(source: &str) -> Vec<String> {
    let mut warnings = Vec::new();

    if source.contains("eval(") {
        warnings.push("Use of eval() detected - may have security implications".to_string());
    }
    if source.contains("Function(") {
        warnings.push("Dynamic Function constructor detected".to_string());
    }
    if source.contains("while(true)") || source.contains("while (true)") {
        warnings.push("Potential infinite loop detected".to_string());
    }
    if source.contains("import(") {
        warnings.push("Dynamic import detected - may not be supported".to_string());
    }

    warnings
}

/// Runtime statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub total_transpilations: u64,
    pub avg_duration_ms: f64,
}

/// The JavaScript runtime.
pub struct JsRuntime {
    config: JsRuntimeConfig,
    stats: RuntimeStats,
}

impl JsRuntime {
    /// Create a new JS runtime.
    pub fn new(config: JsRuntimeConfig) -> Self {
        Self { config, stats: RuntimeStats::default() }
    }

    /// Validate a JS request before execution.
    pub fn validate(&self, request: &JsRequest) -> JsValidation {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check source size
        if request.source.is_empty() {
            errors.push("Source code is empty".to_string());
        }
        if request.source.len() > self.config.max_source_bytes {
            errors.push(format!(
                "Source too large: {} bytes (max: {})",
                request.source.len(),
                self.config.max_source_bytes
            ));
        }

        // Check TypeScript support
        if request.is_typescript && !self.config.enable_typescript {
            errors.push("TypeScript is not enabled in runtime config".to_string());
        }

        // Detect potential issues
        warnings.extend(detect_js_issues(&request.source));

        JsValidation { errors, warnings }
    }

    /// Generate the wrapped script for execution.
    pub fn wrap(&self, request: &JsRequest) -> String {
        generate_wrapper(request, &self.config)
    }

    /// Simulate execution (actual execution requires QuickJS WASM module).
    pub fn execute_simulated(&mut self, request: &JsRequest) -> JsResult {
        let start = Instant::now();

        let validation = self.validate(request);
        if !validation.is_valid() {
            self.stats.total_executions += 1;
            self.stats.failed_executions += 1;
            return JsResult {
                success: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: validation.errors.join("\n"),
                return_value: None,
                duration: start.elapsed(),
                memory_bytes: 0,
                transpiled: false,
                error: Some(JsError {
                    name: "ValidationError".to_string(),
                    message: validation.errors.join("; "),
                    stack: None,
                    line: None,
                    column: None,
                }),
            };
        }

        let wrapped = self.wrap(request);

        self.stats.total_executions += 1;
        self.stats.successful_executions += 1;

        if request.is_typescript {
            self.stats.total_transpilations += 1;
        }

        JsResult {
            success: true,
            exit_code: 0,
            stdout: format!(
                "[simulated] Would execute {} bytes of {}",
                wrapped.len(),
                if request.is_typescript { "TypeScript" } else { "JavaScript" }
            ),
            stderr: String::new(),
            return_value: None,
            duration: start.elapsed(),
            memory_bytes: wrapped.len() as u64,
            transpiled: request.is_typescript,
            error: None,
        }
    }

    /// Get runtime statistics.
    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }

    /// Get the runtime configuration.
    pub fn config(&self) -> &JsRuntimeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_request_new() {
        let req = JsRequest::new("console.log('hello');");
        assert_eq!(req.source, "console.log('hello');");
        assert!(!req.is_typescript);
    }

    #[test]
    fn test_js_request_typescript() {
        let req = JsRequest::typescript("const x: number = 42;");
        assert!(req.is_typescript);
    }

    #[test]
    fn test_js_request_with_input() {
        let req = JsRequest::new("console.log(Isolate.input);").with_input("test data");
        assert_eq!(req.input, Some("test data".to_string()));
    }

    #[test]
    fn test_js_request_with_env() {
        let req = JsRequest::new("console.log(Isolate.env.KEY);").with_env("KEY", "value");
        assert_eq!(req.env.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_validate_valid() {
        let runtime = JsRuntime::new(JsRuntimeConfig::default());
        let req = JsRequest::new("console.log('hello');");
        let result = runtime.validate(&req);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_empty() {
        let runtime = JsRuntime::new(JsRuntimeConfig::default());
        let req = JsRequest::new("");
        let result = runtime.validate(&req);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_too_large() {
        let config = JsRuntimeConfig { max_source_bytes: 10, ..Default::default() };
        let runtime = JsRuntime::new(config);
        let req = JsRequest::new("a".repeat(100));
        let result = runtime.validate(&req);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_typescript_disabled() {
        let config = JsRuntimeConfig { enable_typescript: false, ..Default::default() };
        let runtime = JsRuntime::new(config);
        let req = JsRequest::typescript("const x: number = 42;");
        let result = runtime.validate(&req);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_validate_warnings() {
        let runtime = JsRuntime::new(JsRuntimeConfig::default());
        let req = JsRequest::new("eval('alert(1)');");
        let result = runtime.validate(&req);
        assert!(result.is_valid()); // warnings don't make it invalid
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_generate_wrapper() {
        let config = JsRuntimeConfig::default();
        let req =
            JsRequest::new("console.log('hello');").with_input("my input").with_env("MODE", "test");

        let wrapper = generate_wrapper(&req, &config);
        assert!(wrapper.contains("console"));
        assert!(wrapper.contains("Isolate"));
        assert!(wrapper.contains("my input"));
        assert!(wrapper.contains("MODE"));
        assert!(wrapper.contains("console.log('hello')"));
    }

    #[test]
    fn test_generate_wrapper_no_console() {
        let config = JsRuntimeConfig { enable_console: false, ..Default::default() };
        let req = JsRequest::new("1 + 1;");
        let wrapper = generate_wrapper(&req, &config);
        assert!(!wrapper.contains("__stdout"));
    }

    #[test]
    fn test_execute_simulated() {
        let mut runtime = JsRuntime::new(JsRuntimeConfig::default());
        let req = JsRequest::new("console.log('hello');");

        let result = runtime.execute_simulated(&req);
        assert!(result.success);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("simulated"));
    }

    #[test]
    fn test_execute_simulated_ts() {
        let config = JsRuntimeConfig { enable_typescript: true, ..Default::default() };
        let mut runtime = JsRuntime::new(config);
        let req = JsRequest::typescript("const x: number = 42;");

        let result = runtime.execute_simulated(&req);
        assert!(result.success);
        assert!(result.transpiled);
    }

    #[test]
    fn test_execute_simulated_invalid() {
        let mut runtime = JsRuntime::new(JsRuntimeConfig::default());
        let req = JsRequest::new("");

        let result = runtime.execute_simulated(&req);
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_stats() {
        let mut runtime = JsRuntime::new(JsRuntimeConfig::default());
        runtime.execute_simulated(&JsRequest::new("1 + 1;"));
        runtime.execute_simulated(&JsRequest::new(""));

        let stats = runtime.stats();
        assert_eq!(stats.total_executions, 2);
        assert_eq!(stats.successful_executions, 1);
        assert_eq!(stats.failed_executions, 1);
    }

    #[test]
    fn test_js_error_display() {
        let err = JsError {
            name: "TypeError".to_string(),
            message: "undefined is not a function".to_string(),
            stack: None,
            line: Some(42),
            column: None,
        };
        let s = err.to_string();
        assert!(s.contains("TypeError"));
        assert!(s.contains("line 42"));
    }

    #[test]
    fn test_es_target_display() {
        assert_eq!(EsTarget::Es2022.to_string(), "ES2022");
        assert_eq!(EsTarget::EsNext.to_string(), "ESNext");
    }

    #[test]
    fn test_detect_issues() {
        let warnings = detect_js_issues("eval('code')");
        assert!(!warnings.is_empty());

        let warnings = detect_js_issues("while(true) {}");
        assert!(!warnings.is_empty());

        let warnings = detect_js_issues("console.log('safe')");
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_config_default() {
        let config = JsRuntimeConfig::default();
        assert!(config.enable_console);
        assert!(!config.enable_timers);
        assert!(config.enable_text_codec);
        assert!(!config.enable_typescript);
        assert_eq!(config.max_memory, 128 * 1024 * 1024);
    }

    #[test]
    fn test_host_binding() {
        let binding = HostBinding {
            name: "readFile".to_string(),
            namespace: "isolate.fs".to_string(),
            binding_type: HostBindingType::Async {
                params: vec!["path: string".to_string()],
                return_type: "string".to_string(),
            },
            description: "Read a file".to_string(),
        };
        assert_eq!(binding.name, "readFile");
    }

    #[test]
    fn test_request_serialization() {
        let req = JsRequest::new("console.log(42);").with_env("KEY", "val");
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: JsRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source, "console.log(42);");
    }

    #[test]
    fn test_result_serialization() {
        let result = JsResult {
            success: true,
            exit_code: 0,
            stdout: "hello".to_string(),
            stderr: String::new(),
            return_value: Some("42".to_string()),
            duration: Duration::from_millis(50),
            memory_bytes: 1024,
            transpiled: false,
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("hello"));
    }
}
