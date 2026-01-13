//! Pipeline executor for multi-stage sandbox execution.
//!
//! Runs a sequence of stages where each stage's output feeds into the next stage's input,
//! with per-stage capability scoping and resource budgets.
//!
//! ```rust
//! use isolate_core::workflow::executor::{
//!     PipelineExecutor, PipelineStage, PipelineConfig, StageResult, PipelineResult,
//! };
//! use std::time::Duration;
//!
//! let config = PipelineConfig::default();
//! let executor = PipelineExecutor::new(config);
//!
//! let stages = vec![
//!     PipelineStage::new("validate", "validate.wasm")
//!         .with_timeout(Duration::from_secs(5)),
//!     PipelineStage::new("transform", "transform.wasm")
//!         .with_timeout(Duration::from_secs(30)),
//! ];
//!
//! assert_eq!(stages.len(), 2);
//! assert_eq!(stages[0].name, "validate");
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Configuration for the pipeline executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Maximum number of stages.
    pub max_stages: usize,
    /// Global timeout for the entire pipeline.
    pub global_timeout: Duration,
    /// Whether to stop on first failure.
    pub fail_fast: bool,
    /// Maximum data size between stages (bytes).
    pub max_inter_stage_bytes: usize,
    /// Default per-stage timeout.
    pub default_stage_timeout: Duration,
    /// Default per-stage memory limit.
    pub default_stage_memory: usize,
    /// Default per-stage fuel limit.
    pub default_stage_fuel: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_stages: 20,
            global_timeout: Duration::from_secs(300),
            fail_fast: true,
            max_inter_stage_bytes: 10 * 1024 * 1024, // 10 MB
            default_stage_timeout: Duration::from_secs(30),
            default_stage_memory: 128 * 1024 * 1024, // 128 MB
            default_stage_fuel: 10_000_000,
        }
    }
}

/// A single stage in the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    /// Stage name (unique within pipeline).
    pub name: String,
    /// WASM module path or identifier.
    pub module_ref: String,
    /// Per-stage timeout override.
    pub timeout: Option<Duration>,
    /// Per-stage memory limit override.
    pub memory_limit: Option<usize>,
    /// Per-stage fuel limit override.
    pub fuel_limit: Option<u64>,
    /// Capabilities for this stage.
    pub capabilities: Vec<String>,
    /// Environment variables for this stage.
    pub env: HashMap<String, String>,
    /// Retry policy for this stage.
    pub retry: Option<StageRetryPolicy>,
    /// Transform to apply to output before passing to next stage.
    pub output_transform: Option<OutputTransform>,
}

impl PipelineStage {
    /// Create a new pipeline stage.
    pub fn new(name: impl Into<String>, module_ref: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module_ref: module_ref.into(),
            timeout: None,
            memory_limit: None,
            fuel_limit: None,
            capabilities: Vec::new(),
            env: HashMap::new(),
            retry: None,
            output_transform: None,
        }
    }

    /// Set the timeout for this stage.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the memory limit for this stage.
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = Some(limit);
        self
    }

    /// Set the fuel limit for this stage.
    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.fuel_limit = Some(fuel);
        self
    }

    /// Add a capability.
    pub fn with_capability(mut self, cap: impl Into<String>) -> Self {
        self.capabilities.push(cap.into());
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set retry policy.
    pub fn with_retry(mut self, max_retries: u32, backoff: Duration) -> Self {
        self.retry = Some(StageRetryPolicy { max_retries, backoff, retry_on_exit_codes: vec![1] });
        self
    }

    /// Set output transform.
    pub fn with_output_transform(mut self, transform: OutputTransform) -> Self {
        self.output_transform = Some(transform);
        self
    }
}

/// How to transform stage output before passing to next stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputTransform {
    /// Pass stdout as-is.
    PassThrough,
    /// Extract JSON field from stdout.
    JsonField(String),
    /// Truncate to max bytes.
    Truncate(usize),
    /// Take only first N lines.
    FirstLines(usize),
    /// Take only last N lines.
    LastLines(usize),
}

/// Retry policy for a stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRetryPolicy {
    /// Maximum retries.
    pub max_retries: u32,
    /// Backoff between retries.
    pub backoff: Duration,
    /// Exit codes that trigger retry.
    pub retry_on_exit_codes: Vec<i32>,
}

/// Result of executing a single stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// Stage name.
    pub stage_name: String,
    /// Whether the stage succeeded.
    pub success: bool,
    /// Exit code.
    pub exit_code: i32,
    /// Stage stdout.
    pub stdout: Vec<u8>,
    /// Stage stderr.
    pub stderr: Vec<u8>,
    /// Execution duration.
    pub duration: Duration,
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Peak memory usage.
    pub peak_memory: u64,
    /// Number of retries attempted.
    pub retries: u32,
}

/// Result of executing the entire pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    /// Whether the entire pipeline succeeded.
    pub success: bool,
    /// Per-stage results (in execution order).
    pub stages: Vec<StageResult>,
    /// Total pipeline duration.
    pub total_duration: Duration,
    /// Total fuel consumed across all stages.
    pub total_fuel: u64,
    /// Index of the stage that failed (if any).
    pub failed_stage: Option<usize>,
    /// Final output (stdout of last successful stage).
    pub final_output: Vec<u8>,
}

impl PipelineResult {
    /// Get a stage result by name.
    pub fn stage(&self, name: &str) -> Option<&StageResult> {
        self.stages.iter().find(|s| s.stage_name == name)
    }

    /// Get the number of completed stages.
    pub fn completed_stages(&self) -> usize {
        self.stages.len()
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        let status = if self.success { "SUCCESS" } else { "FAILED" };
        let stages_info: Vec<String> = self
            .stages
            .iter()
            .map(|s| {
                format!(
                    "  {} [{}] {}ms",
                    s.stage_name,
                    if s.success { "OK" } else { "FAIL" },
                    s.duration.as_millis()
                )
            })
            .collect();

        format!(
            "Pipeline {} ({} stages, {}ms total):\n{}",
            status,
            self.stages.len(),
            self.total_duration.as_millis(),
            stages_info.join("\n")
        )
    }
}

/// Apply an output transform to stage output.
pub fn apply_transform(output: &[u8], transform: &OutputTransform) -> Vec<u8> {
    match transform {
        OutputTransform::PassThrough => output.to_vec(),
        OutputTransform::Truncate(max) => {
            if output.len() > *max {
                output[..*max].to_vec()
            } else {
                output.to_vec()
            }
        }
        OutputTransform::JsonField(field) => {
            let text = String::from_utf8_lossy(output);
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(field_value) = value.get(field) {
                    return field_value.to_string().into_bytes();
                }
            }
            output.to_vec()
        }
        OutputTransform::FirstLines(n) => {
            let text = String::from_utf8_lossy(output);
            let lines: Vec<&str> = text.lines().take(*n).collect();
            lines.join("\n").into_bytes()
        }
        OutputTransform::LastLines(n) => {
            let text = String::from_utf8_lossy(output);
            let all_lines: Vec<&str> = text.lines().collect();
            let start = all_lines.len().saturating_sub(*n);
            all_lines[start..].join("\n").into_bytes()
        }
    }
}

/// Validate a pipeline definition before execution.
pub fn validate_pipeline(
    stages: &[PipelineStage],
    config: &PipelineConfig,
) -> Result<(), PipelineValidationError> {
    if stages.is_empty() {
        return Err(PipelineValidationError::EmptyPipeline);
    }

    if stages.len() > config.max_stages {
        return Err(PipelineValidationError::TooManyStages {
            count: stages.len(),
            max: config.max_stages,
        });
    }

    // Check for duplicate stage names
    let mut seen_names = std::collections::HashSet::new();
    for stage in stages {
        if !seen_names.insert(&stage.name) {
            return Err(PipelineValidationError::DuplicateStageName(stage.name.clone()));
        }
        if stage.name.is_empty() {
            return Err(PipelineValidationError::EmptyStageName);
        }
        if stage.module_ref.is_empty() {
            return Err(PipelineValidationError::EmptyModuleRef(stage.name.clone()));
        }
    }

    Ok(())
}

/// Pipeline validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineValidationError {
    EmptyPipeline,
    TooManyStages { count: usize, max: usize },
    DuplicateStageName(String),
    EmptyStageName,
    EmptyModuleRef(String),
}

impl std::fmt::Display for PipelineValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPipeline => write!(f, "Pipeline must have at least one stage"),
            Self::TooManyStages { count, max } => {
                write!(f, "Pipeline has {} stages (max: {})", count, max)
            }
            Self::DuplicateStageName(name) => write!(f, "Duplicate stage name: {}", name),
            Self::EmptyStageName => write!(f, "Stage name cannot be empty"),
            Self::EmptyModuleRef(name) => {
                write!(f, "Stage '{}' has empty module reference", name)
            }
        }
    }
}

/// Pipeline executor.
pub struct PipelineExecutor {
    config: PipelineConfig,
}

impl PipelineExecutor {
    /// Create a new pipeline executor.
    pub fn new(config: PipelineConfig) -> Self {
        Self { config }
    }

    /// Validate a pipeline.
    pub fn validate(&self, stages: &[PipelineStage]) -> Result<(), PipelineValidationError> {
        validate_pipeline(stages, &self.config)
    }

    /// Execute a pipeline with simulated stages (for testing).
    /// In production, this would create actual sandboxes for each stage.
    pub fn execute_simulated(
        &self,
        stages: &[PipelineStage],
        initial_input: &[u8],
    ) -> PipelineResult {
        let start = Instant::now();
        let mut results = Vec::new();
        let mut current_input = initial_input.to_vec();
        let mut total_fuel = 0u64;
        let mut failed_stage = None;

        for (idx, stage) in stages.iter().enumerate() {
            let stage_start = Instant::now();

            // Simulate execution
            let fuel = stage.fuel_limit.unwrap_or(self.config.default_stage_fuel);
            let simulated_output =
                format!("Stage '{}' processed {} bytes of input", stage.name, current_input.len());

            let result = StageResult {
                stage_name: stage.name.clone(),
                success: true,
                exit_code: 0,
                stdout: simulated_output.as_bytes().to_vec(),
                stderr: Vec::new(),
                duration: stage_start.elapsed(),
                fuel_consumed: fuel / 10, // simulated consumption
                peak_memory: 1024 * 1024, // 1MB simulated
                retries: 0,
            };

            total_fuel += result.fuel_consumed;

            // Apply output transform if configured
            let output = if let Some(ref transform) = stage.output_transform {
                apply_transform(&result.stdout, transform)
            } else {
                result.stdout.clone()
            };

            // Check inter-stage data size
            if output.len() > self.config.max_inter_stage_bytes {
                let mut failed_result = result;
                failed_result.success = false;
                failed_result.exit_code = -1;
                failed_result.stderr = format!(
                    "Output size {} exceeds max inter-stage size {}",
                    output.len(),
                    self.config.max_inter_stage_bytes
                )
                .into_bytes();
                results.push(failed_result);
                failed_stage = Some(idx);
                break;
            }

            results.push(result);
            current_input = output;

            // Check global timeout
            if start.elapsed() > self.config.global_timeout {
                failed_stage = Some(idx);
                break;
            }
        }

        let success = failed_stage.is_none();

        PipelineResult {
            success,
            final_output: if success { current_input } else { Vec::new() },
            stages: results,
            total_duration: start.elapsed(),
            total_fuel,
            failed_stage,
        }
    }

    /// Get the executor configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_stage_builder() {
        let stage = PipelineStage::new("validate", "validate.wasm")
            .with_timeout(Duration::from_secs(5))
            .with_memory_limit(64 * 1024 * 1024)
            .with_fuel(500_000)
            .with_capability("stdout")
            .with_env("MODE", "strict");

        assert_eq!(stage.name, "validate");
        assert_eq!(stage.module_ref, "validate.wasm");
        assert_eq!(stage.timeout, Some(Duration::from_secs(5)));
        assert_eq!(stage.memory_limit, Some(64 * 1024 * 1024));
        assert_eq!(stage.fuel_limit, Some(500_000));
        assert!(stage.capabilities.contains(&"stdout".to_string()));
        assert_eq!(stage.env.get("MODE"), Some(&"strict".to_string()));
    }

    #[test]
    fn test_stage_retry_policy() {
        let stage =
            PipelineStage::new("process", "process.wasm").with_retry(3, Duration::from_secs(1));

        let retry = stage.retry.unwrap();
        assert_eq!(retry.max_retries, 3);
        assert_eq!(retry.backoff, Duration::from_secs(1));
    }

    #[test]
    fn test_validate_pipeline_ok() {
        let stages =
            vec![PipelineStage::new("step1", "a.wasm"), PipelineStage::new("step2", "b.wasm")];
        let config = PipelineConfig::default();
        assert!(validate_pipeline(&stages, &config).is_ok());
    }

    #[test]
    fn test_validate_pipeline_empty() {
        let config = PipelineConfig::default();
        assert_eq!(validate_pipeline(&[], &config), Err(PipelineValidationError::EmptyPipeline));
    }

    #[test]
    fn test_validate_pipeline_too_many_stages() {
        let config = PipelineConfig { max_stages: 2, ..Default::default() };
        let stages = vec![
            PipelineStage::new("a", "a.wasm"),
            PipelineStage::new("b", "b.wasm"),
            PipelineStage::new("c", "c.wasm"),
        ];
        assert!(matches!(
            validate_pipeline(&stages, &config),
            Err(PipelineValidationError::TooManyStages { .. })
        ));
    }

    #[test]
    fn test_validate_pipeline_duplicate_name() {
        let config = PipelineConfig::default();
        let stages =
            vec![PipelineStage::new("step1", "a.wasm"), PipelineStage::new("step1", "b.wasm")];
        assert_eq!(
            validate_pipeline(&stages, &config),
            Err(PipelineValidationError::DuplicateStageName("step1".to_string()))
        );
    }

    #[test]
    fn test_validate_pipeline_empty_name() {
        let config = PipelineConfig::default();
        let stages = vec![PipelineStage::new("", "a.wasm")];
        assert_eq!(
            validate_pipeline(&stages, &config),
            Err(PipelineValidationError::EmptyStageName)
        );
    }

    #[test]
    fn test_validate_pipeline_empty_module() {
        let config = PipelineConfig::default();
        let stages = vec![PipelineStage::new("step1", "")];
        assert_eq!(
            validate_pipeline(&stages, &config),
            Err(PipelineValidationError::EmptyModuleRef("step1".to_string()))
        );
    }

    #[test]
    fn test_output_transform_passthrough() {
        let data = b"hello world";
        let result = apply_transform(data, &OutputTransform::PassThrough);
        assert_eq!(result, data);
    }

    #[test]
    fn test_output_transform_truncate() {
        let data = b"hello world";
        let result = apply_transform(data, &OutputTransform::Truncate(5));
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_output_transform_json_field() {
        let data = br#"{"name": "Alice", "age": 30}"#;
        let result = apply_transform(data, &OutputTransform::JsonField("name".to_string()));
        assert_eq!(String::from_utf8(result).unwrap(), "\"Alice\"");
    }

    #[test]
    fn test_output_transform_first_lines() {
        let data = b"line1\nline2\nline3\nline4";
        let result = apply_transform(data, &OutputTransform::FirstLines(2));
        assert_eq!(String::from_utf8(result).unwrap(), "line1\nline2");
    }

    #[test]
    fn test_output_transform_last_lines() {
        let data = b"line1\nline2\nline3\nline4";
        let result = apply_transform(data, &OutputTransform::LastLines(2));
        assert_eq!(String::from_utf8(result).unwrap(), "line3\nline4");
    }

    #[test]
    fn test_pipeline_executor_simulated() {
        let executor = PipelineExecutor::new(PipelineConfig::default());
        let stages = vec![
            PipelineStage::new("validate", "validate.wasm"),
            PipelineStage::new("transform", "transform.wasm"),
            PipelineStage::new("output", "output.wasm"),
        ];

        let result = executor.execute_simulated(&stages, b"input data");
        assert!(result.success);
        assert_eq!(result.completed_stages(), 3);
        assert!(!result.final_output.is_empty());
        assert!(result.total_fuel > 0);
    }

    #[test]
    fn test_pipeline_result_stage_lookup() {
        let executor = PipelineExecutor::new(PipelineConfig::default());
        let stages =
            vec![PipelineStage::new("step1", "a.wasm"), PipelineStage::new("step2", "b.wasm")];

        let result = executor.execute_simulated(&stages, b"");
        assert!(result.stage("step1").is_some());
        assert!(result.stage("step2").is_some());
        assert!(result.stage("nonexistent").is_none());
    }

    #[test]
    fn test_pipeline_result_summary() {
        let executor = PipelineExecutor::new(PipelineConfig::default());
        let stages = vec![PipelineStage::new("step1", "a.wasm")];

        let result = executor.execute_simulated(&stages, b"");
        let summary = result.summary();
        assert!(summary.contains("SUCCESS"));
        assert!(summary.contains("step1"));
    }

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.max_stages, 20);
        assert!(config.fail_fast);
        assert_eq!(config.default_stage_fuel, 10_000_000);
    }

    #[test]
    fn test_validation_error_display() {
        assert_eq!(
            PipelineValidationError::EmptyPipeline.to_string(),
            "Pipeline must have at least one stage"
        );
        assert!(PipelineValidationError::TooManyStages { count: 25, max: 20 }
            .to_string()
            .contains("25"));
    }

    #[test]
    fn test_stage_serialization() {
        let stage = PipelineStage::new("test", "test.wasm").with_timeout(Duration::from_secs(10));
        let json = serde_json::to_string(&stage).unwrap();
        let deserialized: PipelineStage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
    }

    #[test]
    fn test_pipeline_with_transforms() {
        let executor = PipelineExecutor::new(PipelineConfig::default());
        let stages = vec![
            PipelineStage::new("step1", "a.wasm")
                .with_output_transform(OutputTransform::Truncate(50)),
            PipelineStage::new("step2", "b.wasm"),
        ];

        let result = executor.execute_simulated(&stages, b"input");
        assert!(result.success);
    }
}
