//! AI-focused code execution SDK.
//!
//! Provides a high-level, ergonomic wrapper ([`AiSandbox`]) for running
//! LLM-generated code safely. It builds on top of [`Sandbox`] and
//! [`SandboxProfile::AiCodeExecution`] to provide:
//!
//! - Automatic output sanitization (strip ANSI codes, null bytes, control chars)
//! - Execution verdict classification (safe, runtime error, resource exhausted, suspicious)
//! - Configurable retry logic
//! - Output truncation
//!
//! # Example
//!
//! ```no_run
//! use isolate_core::ai_sandbox::{AiSandbox, AiExecutionRequest};
//!
//! # async fn example() -> isolate_core::Result<()> {
//! let sandbox = AiSandbox::builder().build();
//! let wasm_bytes = std::fs::read("ai_generated.wasm")?;
//!
//! let request = AiExecutionRequest::new("req-1", wasm_bytes);
//! let result = sandbox.execute(request).await?;
//!
//! if result.verdict.is_safe() {
//!     println!("Output: {}", result.output);
//! }
//! # Ok(())
//! # }
//! ```

#![allow(missing_docs)]
use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::error::Result;
use crate::sandbox::Sandbox;
use crate::sandbox_profile::SandboxProfile;

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// Classification of an AI code execution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Completed successfully, no issues.
    Safe,
    /// Code had a runtime error (non-zero exit, trapped).
    RuntimeError,
    /// Hit timeout or memory limit.
    ResourceExhausted,
    /// Output contained potentially dangerous patterns (heuristic).
    Suspicious,
}

impl Verdict {
    /// Returns `true` if the verdict is [`Verdict::Safe`].
    pub fn is_safe(&self) -> bool {
        matches!(self, Verdict::Safe)
    }

    /// Returns `true` if the verdict indicates an error
    /// ([`Verdict::RuntimeError`] or [`Verdict::ResourceExhausted`]).
    pub fn is_error(&self) -> bool {
        matches!(self, Verdict::RuntimeError | Verdict::ResourceExhausted)
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => write!(f, "Safe"),
            Self::RuntimeError => write!(f, "RuntimeError"),
            Self::ResourceExhausted => write!(f, "ResourceExhausted"),
            Self::Suspicious => write!(f, "Suspicious"),
        }
    }
}

// ---------------------------------------------------------------------------
// AiExecutionRequest
// ---------------------------------------------------------------------------

/// A request to execute AI-generated WASM code.
pub struct AiExecutionRequest {
    /// Caller-provided identifier for correlation.
    pub request_id: String,
    /// The compiled WASM module bytes.
    pub wasm_bytes: Vec<u8>,
    /// Optional stdin data to feed the module.
    pub stdin: Vec<u8>,
    /// Optional language hint (e.g. "python", "javascript").
    pub language_hint: Option<String>,
    /// Arbitrary metadata (model name, prompt hash, etc.).
    pub metadata: HashMap<String, String>,
}

impl AiExecutionRequest {
    /// Create a new request with the given id and WASM bytes.
    pub fn new(request_id: impl Into<String>, wasm_bytes: Vec<u8>) -> Self {
        Self {
            request_id: request_id.into(),
            wasm_bytes,
            stdin: Vec::new(),
            language_hint: None,
            metadata: HashMap::new(),
        }
    }

    /// Set stdin data.
    pub fn stdin(mut self, data: Vec<u8>) -> Self {
        self.stdin = data;
        self
    }

    /// Set a language hint.
    pub fn language_hint(mut self, hint: impl Into<String>) -> Self {
        self.language_hint = Some(hint.into());
        self
    }

    /// Insert a metadata key-value pair.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// AiExecutionResult
// ---------------------------------------------------------------------------

/// The result of an AI code execution.
pub struct AiExecutionResult {
    /// The request id echoed back.
    pub request_id: String,
    /// Sanitized stdout.
    pub output: String,
    /// Sanitized stderr.
    pub errors: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Wall-clock execution time.
    pub execution_time: Duration,
    /// Peak memory used in bytes.
    pub memory_used: usize,
    /// Verdict classification.
    pub verdict: Verdict,
    /// Which attempt succeeded (1-based).
    pub attempt: u32,
}

// ---------------------------------------------------------------------------
// AiSandboxBuilder
// ---------------------------------------------------------------------------

/// Builder for [`AiSandbox`].
#[derive(Debug)]
#[must_use = "builders do nothing unless you call .build()"]
pub struct AiSandboxBuilder {
    max_retries: u32,
    default_timeout: Duration,
    default_memory: usize,
    output_limit: usize,
    sanitize_output: bool,
}

impl Default for AiSandboxBuilder {
    fn default() -> Self {
        Self {
            max_retries: 0,
            default_timeout: Duration::from_secs(10),
            default_memory: 32 * 1024 * 1024, // 32 MB
            output_limit: 1024 * 1024,        // 1 MB
            sanitize_output: true,
        }
    }
}

impl AiSandboxBuilder {
    /// Set maximum retry count.
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Set default wall-clock timeout.
    pub fn timeout(mut self, d: Duration) -> Self {
        self.default_timeout = d;
        self
    }

    /// Set default memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.default_memory = bytes;
        self
    }

    /// Set maximum output bytes before truncation.
    pub fn output_limit(mut self, bytes: usize) -> Self {
        self.output_limit = bytes;
        self
    }

    /// Enable or disable output sanitization.
    pub fn sanitize(mut self, enable: bool) -> Self {
        self.sanitize_output = enable;
        self
    }

    /// Build the [`AiSandbox`].
    pub fn build(self) -> AiSandbox {
        AiSandbox {
            max_retries: self.max_retries,
            default_timeout: self.default_timeout,
            default_memory: self.default_memory,
            output_limit: self.output_limit,
            sanitize_output: self.sanitize_output,
        }
    }
}

// ---------------------------------------------------------------------------
// AiSandbox
// ---------------------------------------------------------------------------

/// High-level, ergonomic wrapper for running LLM-generated code.
pub struct AiSandbox {
    max_retries: u32,
    default_timeout: Duration,
    default_memory: usize,
    output_limit: usize,
    sanitize_output: bool,
}

impl AiSandbox {
    /// Create a new builder with sensible defaults.
    pub fn builder() -> AiSandboxBuilder {
        AiSandboxBuilder::default()
    }

    /// Execute an AI-generated WASM module.
    ///
    /// Uses [`SandboxProfile::AiCodeExecution`] as the base profile, then
    /// overrides with the configured timeout and memory limit. Retries up to
    /// `max_retries` times on transient errors.
    pub async fn execute(&self, request: AiExecutionRequest) -> Result<AiExecutionResult> {
        let mut last_err: Option<crate::error::Error> = None;

        for attempt in 1..=(self.max_retries + 1) {
            let config = SandboxConfig::builder()
                .module(&request.wasm_bytes)?
                .use_profile(SandboxProfile::AiCodeExecution)
                .wall_time_limit(self.default_timeout)
                .memory_limit(self.default_memory)
                .capability(Capability::stdin())
                .build()?;

            let mut sandbox = Sandbox::create(config).await?;
            let run_result = sandbox.run(&request.stdin).await;

            match run_result {
                Ok(output) => {
                    let stdout_raw = output.stdout_str();
                    let stderr_raw = output.stderr_str();

                    let stdout = if self.sanitize_output {
                        self.sanitize_output(&stdout_raw)
                    } else {
                        stdout_raw
                    };
                    let stderr = if self.sanitize_output {
                        self.sanitize_output(&stderr_raw)
                    } else {
                        stderr_raw
                    };

                    let timed_out = false;
                    let verdict =
                        self.classify_verdict(output.exit_code, &stdout, &stderr, timed_out);

                    return Ok(AiExecutionResult {
                        request_id: request.request_id.clone(),
                        output: stdout,
                        errors: stderr,
                        exit_code: output.exit_code,
                        execution_time: output.duration,
                        memory_used: output.resource_usage.peak_memory,
                        verdict,
                        attempt,
                    });
                }
                Err(e) => {
                    if e.is_timeout() {
                        return Ok(AiExecutionResult {
                            request_id: request.request_id.clone(),
                            output: String::new(),
                            errors: e.to_string(),
                            exit_code: -1,
                            execution_time: self.default_timeout,
                            memory_used: 0,
                            verdict: Verdict::ResourceExhausted,
                            attempt,
                        });
                    }
                    if e.is_resource_limit() {
                        return Ok(AiExecutionResult {
                            request_id: request.request_id.clone(),
                            output: String::new(),
                            errors: e.to_string(),
                            exit_code: -1,
                            execution_time: Duration::ZERO,
                            memory_used: 0,
                            verdict: Verdict::ResourceExhausted,
                            attempt,
                        });
                    }
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap())
    }

    /// Strip ANSI escape codes, null bytes, and non-printable control chars
    /// (keeping `\n` and `\t`), then truncate to [`output_limit`].
    pub fn sanitize_output(&self, raw: &str) -> String {
        let mut result = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();

        while let Some(ch) = chars.next() {
            // Strip ANSI escape sequences: ESC [ ... final_byte
            if ch == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                                  // consume until a letter (0x40–0x7E) terminates the sequence
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                    continue;
                }
                // bare ESC — skip it
                continue;
            }
            // Keep newline and tab
            if ch == '\n' || ch == '\t' {
                result.push(ch);
                continue;
            }
            // Strip null bytes and other control characters
            if ch.is_control() {
                continue;
            }
            result.push(ch);
        }

        // Truncate to output_limit (byte-aware)
        if result.len() > self.output_limit {
            let mut end = self.output_limit;
            while end > 0 && !result.is_char_boundary(end) {
                end -= 1;
            }
            result.truncate(end);
        }

        result
    }

    /// Classify the execution verdict based on exit code, outputs, and timeout.
    pub fn classify_verdict(
        &self,
        exit_code: i32,
        _stdout: &str,
        stderr: &str,
        timed_out: bool,
    ) -> Verdict {
        if timed_out {
            return Verdict::ResourceExhausted;
        }
        if exit_code != 0 {
            return Verdict::RuntimeError;
        }

        const SUSPICIOUS_PATTERNS: &[&str] = &["rm -rf", "sudo", "/etc/passwd", "eval(", "exec("];

        let stderr_lower = stderr.to_lowercase();
        for pattern in SUSPICIOUS_PATTERNS {
            if stderr_lower.contains(&pattern.to_lowercase()) {
                return Verdict::Suspicious;
            }
        }

        Verdict::Safe
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- builder defaults ---------------------------------------------------

    #[test]
    fn test_builder_defaults() {
        let sb = AiSandbox::builder().build();
        assert_eq!(sb.max_retries, 0);
        assert_eq!(sb.default_timeout, Duration::from_secs(10));
        assert_eq!(sb.default_memory, 32 * 1024 * 1024);
        assert_eq!(sb.output_limit, 1024 * 1024);
        assert!(sb.sanitize_output);
    }

    #[test]
    fn test_builder_customization() {
        let sb = AiSandbox::builder()
            .max_retries(3)
            .timeout(Duration::from_secs(30))
            .memory_limit(64 * 1024 * 1024)
            .output_limit(2048)
            .sanitize(false)
            .build();

        assert_eq!(sb.max_retries, 3);
        assert_eq!(sb.default_timeout, Duration::from_secs(30));
        assert_eq!(sb.default_memory, 64 * 1024 * 1024);
        assert_eq!(sb.output_limit, 2048);
        assert!(!sb.sanitize_output);
    }

    // -- sanitize_output ----------------------------------------------------

    #[test]
    fn test_sanitize_output_strips_ansi() {
        let sb = AiSandbox::builder().build();
        let raw = "\x1b[31mhello\x1b[0m world";
        assert_eq!(sb.sanitize_output(raw), "hello world");
    }

    #[test]
    fn test_sanitize_output_truncates() {
        let sb = AiSandbox::builder().output_limit(5).build();
        let raw = "abcdefghij";
        assert_eq!(sb.sanitize_output(raw), "abcde");
    }

    #[test]
    fn test_sanitize_output_preserves_newlines_and_tabs() {
        let sb = AiSandbox::builder().build();
        let raw = "line1\n\tline2\n";
        assert_eq!(sb.sanitize_output(raw), "line1\n\tline2\n");
    }

    #[test]
    fn test_sanitize_output_strips_null_bytes() {
        let sb = AiSandbox::builder().build();
        let raw = "abc\0def";
        assert_eq!(sb.sanitize_output(raw), "abcdef");
    }

    // -- verdict classification ---------------------------------------------

    #[test]
    fn test_verdict_safe() {
        let sb = AiSandbox::builder().build();
        assert_eq!(sb.classify_verdict(0, "ok", "", false), Verdict::Safe);
    }

    #[test]
    fn test_verdict_runtime_error() {
        let sb = AiSandbox::builder().build();
        assert_eq!(sb.classify_verdict(1, "", "panic", false), Verdict::RuntimeError);
    }

    #[test]
    fn test_verdict_resource_exhausted() {
        let sb = AiSandbox::builder().build();
        assert_eq!(sb.classify_verdict(0, "", "", true), Verdict::ResourceExhausted);
    }

    #[test]
    fn test_verdict_suspicious_patterns() {
        let sb = AiSandbox::builder().build();
        assert_eq!(sb.classify_verdict(0, "", "tried rm -rf /", false), Verdict::Suspicious);
        assert_eq!(sb.classify_verdict(0, "", "using sudo command", false), Verdict::Suspicious);
        assert_eq!(sb.classify_verdict(0, "", "reading /etc/passwd", false), Verdict::Suspicious);
        assert_eq!(sb.classify_verdict(0, "", "eval(user_input)", false), Verdict::Suspicious);
        assert_eq!(sb.classify_verdict(0, "", "exec(cmd)", false), Verdict::Suspicious);
    }

    // -- Verdict helpers ----------------------------------------------------

    #[test]
    fn test_verdict_is_safe_and_is_error() {
        assert!(Verdict::Safe.is_safe());
        assert!(!Verdict::Safe.is_error());

        assert!(!Verdict::RuntimeError.is_safe());
        assert!(Verdict::RuntimeError.is_error());

        assert!(!Verdict::ResourceExhausted.is_safe());
        assert!(Verdict::ResourceExhausted.is_error());

        assert!(!Verdict::Suspicious.is_safe());
        assert!(!Verdict::Suspicious.is_error());
    }

    // -- Verdict Display ----------------------------------------------------

    #[test]
    fn test_verdict_display() {
        assert_eq!(Verdict::Safe.to_string(), "Safe");
        assert_eq!(Verdict::RuntimeError.to_string(), "RuntimeError");
        assert_eq!(Verdict::ResourceExhausted.to_string(), "ResourceExhausted");
        assert_eq!(Verdict::Suspicious.to_string(), "Suspicious");
    }

    // -- AiExecutionRequest -------------------------------------------------

    #[test]
    fn test_request_creation_and_metadata() {
        let req = AiExecutionRequest::new("r1", vec![1, 2, 3])
            .language_hint("python")
            .stdin(b"input".to_vec())
            .metadata("model", "gpt-4")
            .metadata("prompt_hash", "abc123");

        assert_eq!(req.request_id, "r1");
        assert_eq!(req.wasm_bytes, vec![1, 2, 3]);
        assert_eq!(req.stdin, b"input");
        assert_eq!(req.language_hint.as_deref(), Some("python"));
        assert_eq!(req.metadata.get("model").unwrap(), "gpt-4");
        assert_eq!(req.metadata.get("prompt_hash").unwrap(), "abc123");
    }

    // -- Integration: execute with fixtures ---------------------------------

    const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");
    const EXIT_42_WASM: &[u8] = include_bytes!("../tests/fixtures/exit_42.wasm");

    #[tokio::test]
    async fn test_execute_hello_wasm() {
        let sb = AiSandbox::builder().build();
        let req = AiExecutionRequest::new("hello-1", HELLO_WASM.to_vec());
        let result = sb.execute(req).await.expect("execution should succeed");

        assert_eq!(result.request_id, "hello-1");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.verdict, Verdict::Safe);
        assert!(result.output.contains("Hello from WASM!"));
        assert_eq!(result.attempt, 1);
    }

    #[tokio::test]
    async fn test_execute_exit_42_wasm() {
        let sb = AiSandbox::builder().build();
        let req = AiExecutionRequest::new("exit42-1", EXIT_42_WASM.to_vec());
        let result = sb.execute(req).await.expect("execution should succeed");

        assert_eq!(result.request_id, "exit42-1");
        assert_eq!(result.exit_code, 42);
        assert_eq!(result.verdict, Verdict::RuntimeError);
        assert_eq!(result.attempt, 1);
    }
}
