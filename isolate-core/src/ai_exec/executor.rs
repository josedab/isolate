//! AI code executor implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Supported programming languages for AI code execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Python code.
    Python,
    /// JavaScript code.
    JavaScript,
    /// TypeScript code.
    TypeScript,
    /// Rust code (pre-compiled to WASM).
    Rust,
    /// C code (pre-compiled to WASM).
    C,
    /// C++ code (pre-compiled to WASM).
    Cpp,
    /// AssemblyScript code.
    AssemblyScript,
    /// Go code (via TinyGo WASM).
    Go,
    /// Pre-compiled WASM binary.
    Wasm,
}

impl Language {
    /// Detect language from source code heuristics.
    pub fn detect(source: &str) -> Option<Self> {
        let trimmed = source.trim();

        // Python indicators
        if trimmed.starts_with("import ")
            || trimmed.starts_with("from ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("class ")
            || trimmed.contains("print(")
            || trimmed.starts_with("#!") && trimmed.contains("python")
        {
            return Some(Self::Python);
        }

        // JavaScript/TypeScript indicators
        if trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
            || trimmed.starts_with("function ")
            || trimmed.contains("console.log")
            || trimmed.starts_with("export ")
        {
            if trimmed.contains(": string")
                || trimmed.contains(": number")
                || trimmed.contains(": boolean")
                || trimmed.contains("interface ")
            {
                return Some(Self::TypeScript);
            }
            return Some(Self::JavaScript);
        }

        // Rust indicators
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("mod ")
            || trimmed.contains("fn main()")
            || trimmed.contains("println!")
        {
            return Some(Self::Rust);
        }

        // C/C++ indicators
        if trimmed.starts_with("#include") {
            if trimmed.contains("<iostream>")
                || trimmed.contains("std::")
                || trimmed.contains("cout")
            {
                return Some(Self::Cpp);
            }
            return Some(Self::C);
        }

        // Go indicators
        if trimmed.starts_with("package ")
            || trimmed.contains("func main()")
            || trimmed.contains("fmt.Println")
        {
            return Some(Self::Go);
        }

        None
    }

    /// Get the file extension for this language.
    pub fn extension(&self) -> &str {
        match self {
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
            Self::Rust => "rs",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::AssemblyScript => "ts",
            Self::Go => "go",
            Self::Wasm => "wasm",
        }
    }

    /// Get the MIME type for this language.
    pub fn mime_type(&self) -> &str {
        match self {
            Self::Python => "text/x-python",
            Self::JavaScript => "text/javascript",
            Self::TypeScript => "text/typescript",
            Self::Rust => "text/x-rust",
            Self::C => "text/x-c",
            Self::Cpp => "text/x-c++",
            Self::AssemblyScript => "text/typescript",
            Self::Go => "text/x-go",
            Self::Wasm => "application/wasm",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::JavaScript => write!(f, "javascript"),
            Self::TypeScript => write!(f, "typescript"),
            Self::Rust => write!(f, "rust"),
            Self::C => write!(f, "c"),
            Self::Cpp => write!(f, "c++"),
            Self::AssemblyScript => write!(f, "assemblyscript"),
            Self::Go => write!(f, "go"),
            Self::Wasm => write!(f, "wasm"),
        }
    }
}

/// Safety level for AI code execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    /// Maximum restrictions: no I/O, minimal resources.
    Strict,
    /// Standard restrictions: stdout only, moderate resources.
    Standard,
    /// Relaxed restrictions: stdio + filesystem read, more resources.
    Relaxed,
    /// Custom safety configuration.
    Custom,
}

/// Pre-configured execution profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionProfile {
    /// Profile name.
    pub name: String,
    /// Safety level.
    pub safety_level: SafetyLevel,
    /// Maximum memory in bytes.
    pub max_memory: usize,
    /// Maximum CPU fuel.
    pub max_fuel: u64,
    /// Maximum execution time.
    pub max_duration: Duration,
    /// Maximum output size in bytes.
    pub max_output_bytes: usize,
    /// Maximum input size in bytes.
    pub max_input_bytes: usize,
    /// Allow stdout.
    pub allow_stdout: bool,
    /// Allow stderr.
    pub allow_stderr: bool,
    /// Allow stdin.
    pub allow_stdin: bool,
    /// Allow filesystem read paths.
    pub allow_fs_read: Vec<String>,
    /// Allow network hosts.
    pub allow_network: Vec<String>,
    /// Enable output sanitization.
    pub sanitize_output: bool,
}

impl ExecutionProfile {
    /// Conservative profile for maximum safety.
    pub fn conservative() -> Self {
        Self {
            name: "conservative".to_string(),
            safety_level: SafetyLevel::Strict,
            max_memory: 32 * 1024 * 1024, // 32 MB
            max_fuel: 1_000_000,
            max_duration: Duration::from_secs(5),
            max_output_bytes: 64 * 1024,  // 64 KB
            max_input_bytes: 1024 * 1024, // 1 MB
            allow_stdout: true,
            allow_stderr: true,
            allow_stdin: false,
            allow_fs_read: Vec::new(),
            allow_network: Vec::new(),
            sanitize_output: true,
        }
    }

    /// Standard profile for typical AI code execution.
    pub fn standard() -> Self {
        Self {
            name: "standard".to_string(),
            safety_level: SafetyLevel::Standard,
            max_memory: 128 * 1024 * 1024, // 128 MB
            max_fuel: 10_000_000,
            max_duration: Duration::from_secs(30),
            max_output_bytes: 1024 * 1024,     // 1 MB
            max_input_bytes: 10 * 1024 * 1024, // 10 MB
            allow_stdout: true,
            allow_stderr: true,
            allow_stdin: true,
            allow_fs_read: Vec::new(),
            allow_network: Vec::new(),
            sanitize_output: true,
        }
    }

    /// Permissive profile for trusted environments.
    pub fn permissive() -> Self {
        Self {
            name: "permissive".to_string(),
            safety_level: SafetyLevel::Relaxed,
            max_memory: 512 * 1024 * 1024, // 512 MB
            max_fuel: 100_000_000,
            max_duration: Duration::from_secs(300),
            max_output_bytes: 10 * 1024 * 1024, // 10 MB
            max_input_bytes: 50 * 1024 * 1024,  // 50 MB
            allow_stdout: true,
            allow_stderr: true,
            allow_stdin: true,
            allow_fs_read: vec!["/data".to_string()],
            allow_network: Vec::new(),
            sanitize_output: false,
        }
    }
}

/// A request to execute AI-generated code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRequest {
    /// Source code to execute.
    pub source: String,
    /// Language (auto-detected if None).
    pub language: Option<Language>,
    /// Input data for the program.
    #[serde(default)]
    pub input: Option<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Request metadata (for tracking/billing).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl CodeRequest {
    /// Create a new code request.
    pub fn new(source: impl Into<String>, language: Language) -> Self {
        Self {
            source: source.into(),
            language: Some(language),
            input: None,
            env: HashMap::new(),
            args: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a code request with auto-detection.
    pub fn auto_detect(source: impl Into<String>) -> Self {
        let source = source.into();
        let language = Language::detect(&source);
        Self {
            source,
            language,
            input: None,
            env: HashMap::new(),
            args: Vec::new(),
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

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get the detected or specified language.
    pub fn resolved_language(&self) -> Option<Language> {
        self.language.or_else(|| Language::detect(&self.source))
    }
}

/// Result of AI code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeResult {
    /// Whether execution succeeded (exit code 0).
    pub success: bool,
    /// Exit code.
    pub exit_code: i32,
    /// Stdout output.
    pub stdout: String,
    /// Stderr output.
    pub stderr: String,
    /// Execution duration.
    pub duration: Duration,
    /// Language used for execution.
    pub language: Language,
    /// Resource cost information.
    pub cost: CostEstimate,
    /// Safety checks performed.
    pub safety_checks: Vec<SafetyCheck>,
    /// Whether output was truncated.
    pub output_truncated: bool,
    /// Whether output was sanitized.
    pub output_sanitized: bool,
}

/// Cost estimate for execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Total I/O bytes.
    pub io_bytes: u64,
    /// Wall time in milliseconds.
    pub wall_time_ms: f64,
    /// Estimated cost units (for billing).
    pub cost_units: f64,
}

impl CostEstimate {
    /// Estimate cost from a code request before execution.
    pub fn pre_estimate(source: &str, profile: &ExecutionProfile) -> Self {
        let source_len = source.len();

        // Heuristic: longer code likely uses more fuel
        let estimated_fuel = (source_len as u64 * 100).min(profile.max_fuel);
        let estimated_memory = (source_len * 1024).min(profile.max_memory) as u64;

        Self {
            fuel_consumed: estimated_fuel,
            peak_memory_bytes: estimated_memory,
            io_bytes: source_len as u64,
            wall_time_ms: (source_len as f64 / 1000.0).max(10.0),
            cost_units: estimated_fuel as f64 / 1_000_000.0,
        }
    }
}

/// A safety check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    /// Check name.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Description of what was checked.
    pub description: String,
    /// Severity if failed (info, warning, error).
    pub severity: String,
}

/// Output sanitization configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeConfig {
    /// Maximum output length in bytes.
    pub max_output_bytes: usize,
    /// Strip ANSI escape codes.
    pub strip_ansi: bool,
    /// Strip null bytes.
    pub strip_null_bytes: bool,
    /// Replace non-UTF8 sequences.
    pub ensure_utf8: bool,
    /// Patterns to redact (e.g., email addresses, API keys).
    pub redact_patterns: Vec<String>,
}

impl Default for SanitizeConfig {
    fn default() -> Self {
        Self {
            max_output_bytes: 1024 * 1024, // 1 MB
            strip_ansi: true,
            strip_null_bytes: true,
            ensure_utf8: true,
            redact_patterns: Vec::new(),
        }
    }
}

/// Output sanitizer.
pub struct OutputSanitizer {
    config: SanitizeConfig,
}

impl OutputSanitizer {
    /// Create a new sanitizer.
    pub fn new(config: SanitizeConfig) -> Self {
        Self { config }
    }

    /// Sanitize output bytes, returning a clean UTF-8 string.
    pub fn sanitize(&self, output: &[u8]) -> (String, bool) {
        let truncated = output.len() > self.config.max_output_bytes;
        let bytes = if truncated { &output[..self.config.max_output_bytes] } else { output };

        let mut text = if self.config.ensure_utf8 {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            // Try strict conversion, fall back to lossy
            String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
        };

        if self.config.strip_null_bytes {
            text = text.replace('\0', "");
        }

        if self.config.strip_ansi {
            text = strip_ansi_codes(&text);
        }

        (text, truncated)
    }
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC sequences
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                              // Skip until we hit a letter
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// The AI code executor.
pub struct CodeExecutor {
    profile: ExecutionProfile,
    sanitizer: OutputSanitizer,
}

impl CodeExecutor {
    /// Create a new code executor with the given profile.
    pub fn new(profile: ExecutionProfile) -> Self {
        let sanitize_config =
            SanitizeConfig { max_output_bytes: profile.max_output_bytes, ..Default::default() };
        Self { profile, sanitizer: OutputSanitizer::new(sanitize_config) }
    }

    /// Get the execution profile.
    pub fn profile(&self) -> &ExecutionProfile {
        &self.profile
    }

    /// Perform pre-execution safety checks on the code request.
    pub fn pre_check(&self, request: &CodeRequest) -> Vec<SafetyCheck> {
        let mut checks = Vec::new();

        // Check source code size
        checks.push(SafetyCheck {
            name: "source_size".to_string(),
            passed: request.source.len() <= self.profile.max_input_bytes,
            description: format!(
                "Source code size {} bytes (max: {} bytes)",
                request.source.len(),
                self.profile.max_input_bytes
            ),
            severity: "error".to_string(),
        });

        // Check language is supported
        let lang = request.resolved_language();
        checks.push(SafetyCheck {
            name: "language_detected".to_string(),
            passed: lang.is_some(),
            description: match lang {
                Some(l) => format!("Detected language: {}", l),
                None => "Could not detect source language".to_string(),
            },
            severity: "error".to_string(),
        });

        // Check for suspicious patterns
        let suspicious = check_suspicious_patterns(&request.source);
        checks.push(SafetyCheck {
            name: "suspicious_patterns".to_string(),
            passed: suspicious.is_empty(),
            description: if suspicious.is_empty() {
                "No suspicious patterns detected".to_string()
            } else {
                format!("Suspicious patterns: {}", suspicious.join(", "))
            },
            severity: "warning".to_string(),
        });

        // Check input size
        if let Some(ref input) = request.input {
            checks.push(SafetyCheck {
                name: "input_size".to_string(),
                passed: input.len() <= self.profile.max_input_bytes,
                description: format!(
                    "Input size {} bytes (max: {} bytes)",
                    input.len(),
                    self.profile.max_input_bytes
                ),
                severity: "error".to_string(),
            });
        }

        checks
    }

    /// Estimate the cost of executing a code request.
    pub fn estimate_cost(&self, request: &CodeRequest) -> CostEstimate {
        CostEstimate::pre_estimate(&request.source, &self.profile)
    }

    /// Sanitize execution output.
    pub fn sanitize_output(&self, output: &[u8]) -> (String, bool) {
        self.sanitizer.sanitize(output)
    }
}

/// Check for suspicious patterns in source code.
fn check_suspicious_patterns(source: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let lower = source.to_lowercase();

    // Check for system access attempts
    let patterns = [
        ("os.system", "System command execution"),
        ("subprocess", "Subprocess execution"),
        ("eval(", "Dynamic code evaluation"),
        ("exec(", "Dynamic code execution"),
        ("__import__", "Dynamic import"),
        ("open('/etc", "System file access"),
        ("open('/proc", "Proc filesystem access"),
        ("/dev/", "Device access"),
        ("rm -rf", "Destructive file operation"),
        ("wget ", "Network download"),
        ("curl ", "Network request"),
        ("socket.socket", "Raw socket creation"),
        ("ctypes", "FFI access"),
    ];

    for (pattern, description) in &patterns {
        if lower.contains(pattern) {
            findings.push(description.to_string());
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detect_python() {
        assert_eq!(Language::detect("import os\nprint('hello')"), Some(Language::Python));
        assert_eq!(Language::detect("def foo():\n    pass"), Some(Language::Python));
        assert_eq!(Language::detect("from typing import List"), Some(Language::Python));
    }

    #[test]
    fn test_language_detect_javascript() {
        assert_eq!(Language::detect("const x = 1;\nconsole.log(x);"), Some(Language::JavaScript));
        assert_eq!(
            Language::detect("function hello() { return 'hi'; }"),
            Some(Language::JavaScript)
        );
    }

    #[test]
    fn test_language_detect_typescript() {
        assert_eq!(
            Language::detect("const x: string = 'hello';\nconsole.log(x);"),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::detect("let items: number[] = [1, 2, 3];"),
            Some(Language::TypeScript)
        );
    }

    #[test]
    fn test_language_detect_rust() {
        assert_eq!(
            Language::detect("fn main() {\n    println!(\"hello\");\n}"),
            Some(Language::Rust)
        );
    }

    #[test]
    fn test_language_detect_c() {
        assert_eq!(Language::detect("#include <stdio.h>\nint main() {}"), Some(Language::C));
    }

    #[test]
    fn test_language_detect_cpp() {
        assert_eq!(
            Language::detect("#include <iostream>\nusing namespace std;"),
            Some(Language::Cpp)
        );
    }

    #[test]
    fn test_language_detect_go() {
        assert_eq!(
            Language::detect("package main\nimport \"fmt\"\nfunc main() {}"),
            Some(Language::Go)
        );
    }

    #[test]
    fn test_language_detect_unknown() {
        assert_eq!(Language::detect("just some text"), None);
    }

    #[test]
    fn test_execution_profiles() {
        let conservative = ExecutionProfile::conservative();
        assert_eq!(conservative.safety_level, SafetyLevel::Strict);
        assert_eq!(conservative.max_memory, 32 * 1024 * 1024);
        assert!(conservative.sanitize_output);

        let standard = ExecutionProfile::standard();
        assert_eq!(standard.safety_level, SafetyLevel::Standard);
        assert_eq!(standard.max_memory, 128 * 1024 * 1024);

        let permissive = ExecutionProfile::permissive();
        assert_eq!(permissive.safety_level, SafetyLevel::Relaxed);
        assert!(!permissive.sanitize_output);
    }

    #[test]
    fn test_code_request() {
        let req = CodeRequest::new("print('hello')", Language::Python)
            .with_input("test input")
            .with_env("KEY", "value")
            .with_metadata("request_id", "abc-123");

        assert_eq!(req.source, "print('hello')");
        assert_eq!(req.language, Some(Language::Python));
        assert_eq!(req.input, Some("test input".to_string()));
        assert_eq!(req.env.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_code_request_auto_detect() {
        let req = CodeRequest::auto_detect("print('hello')");
        assert_eq!(req.resolved_language(), Some(Language::Python));

        let req = CodeRequest::auto_detect("console.log('hi')");
        assert_eq!(req.resolved_language(), Some(Language::JavaScript));
    }

    #[test]
    fn test_cost_estimate() {
        let profile = ExecutionProfile::standard();
        let estimate = CostEstimate::pre_estimate("print('hello')", &profile);

        assert!(estimate.fuel_consumed > 0);
        assert!(estimate.cost_units > 0.0);
    }

    #[test]
    fn test_pre_check() {
        let executor = CodeExecutor::new(ExecutionProfile::standard());
        let req = CodeRequest::new("print('hello')", Language::Python);

        let checks = executor.pre_check(&req);
        assert!(checks.iter().all(|c| c.passed));
    }

    #[test]
    fn test_pre_check_suspicious() {
        let executor = CodeExecutor::new(ExecutionProfile::conservative());
        let req = CodeRequest::new("import os\nos.system('rm -rf /')", Language::Python);

        let checks = executor.pre_check(&req);
        let suspicious = checks.iter().find(|c| c.name == "suspicious_patterns").unwrap();
        assert!(!suspicious.passed);
    }

    #[test]
    fn test_output_sanitizer() {
        let sanitizer = OutputSanitizer::new(SanitizeConfig::default());

        // Normal output
        let (text, truncated) = sanitizer.sanitize(b"Hello, world!");
        assert_eq!(text, "Hello, world!");
        assert!(!truncated);

        // Strip null bytes
        let (text, _) = sanitizer.sanitize(b"hello\0world");
        assert_eq!(text, "helloworld");

        // Strip ANSI codes
        let (text, _) = sanitizer.sanitize(b"\x1b[31mred\x1b[0m text");
        assert_eq!(text, "red text");
    }

    #[test]
    fn test_output_sanitizer_truncation() {
        let config = SanitizeConfig { max_output_bytes: 10, ..Default::default() };
        let sanitizer = OutputSanitizer::new(config);

        let (text, truncated) = sanitizer.sanitize(b"this is a very long string");
        assert!(truncated);
        assert!(text.len() <= 10);
    }

    #[test]
    fn test_suspicious_patterns() {
        let findings = check_suspicious_patterns("import os\nos.system('whoami')");
        assert!(!findings.is_empty());

        let findings = check_suspicious_patterns("print('hello world')");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_language_display() {
        assert_eq!(Language::Python.to_string(), "python");
        assert_eq!(Language::JavaScript.to_string(), "javascript");
        assert_eq!(Language::Rust.to_string(), "rust");
    }

    #[test]
    fn test_language_extension() {
        assert_eq!(Language::Python.extension(), "py");
        assert_eq!(Language::JavaScript.extension(), "js");
        assert_eq!(Language::Rust.extension(), "rs");
    }
}
