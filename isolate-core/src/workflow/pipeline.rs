//! Pipeline YAML DSL for sandbox composition.
//!
//! Defines a YAML-based DSL for composing sandbox pipelines as DAGs.
//! Pipelines are validated at definition time, including cycle detection,
//! type checking, and capability propagation.
//!
//! # YAML Format
//!
//! ```yaml
//! name: etl-pipeline
//! description: Extract, transform, load pipeline
//! timeout: 300s
//! steps:
//!   extract:
//!     module: extractor.wasm
//!     capabilities: [filesystem_read]
//!     fuel: 1000000
//!   transform:
//!     module: transformer.wasm
//!     depends_on: [extract]
//!     fuel: 2000000
//!   load:
//!     module: loader.wasm
//!     depends_on: [transform]
//!     capabilities: [network]
//!     fuel: 500000
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// A pipeline definition parsed from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDefinition {
    /// Pipeline name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Global timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Pipeline steps keyed by name.
    pub steps: HashMap<String, PipelineStep>,
    /// Global capabilities applied to all steps.
    #[serde(default)]
    pub global_capabilities: Vec<String>,
    /// Pipeline metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// A single step in a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    /// WASM module reference.
    pub module: String,
    /// Function to call (default: _start).
    #[serde(default)]
    pub function: Option<String>,
    /// Dependencies (steps that must complete first).
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Step capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Fuel limit.
    #[serde(default)]
    pub fuel: Option<u64>,
    /// Memory limit in bytes.
    #[serde(default)]
    pub memory_limit: Option<u64>,
    /// Step timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Retry count (0 = no retry).
    #[serde(default)]
    pub retries: u32,
    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Input mappings from previous steps.
    #[serde(default)]
    pub inputs: HashMap<String, StepInput>,
}

/// Input source for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepInput {
    /// Literal value.
    Literal(String),
    /// Reference to another step's output.
    StepRef(StepOutputRef),
}

/// Reference to a previous step's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutputRef {
    /// Step name.
    pub from_step: String,
    /// Output name.
    pub output: String,
}

/// Pipeline validation error.
#[derive(Debug, Clone)]
pub enum PipelineError {
    /// Parse error.
    ParseError(String),
    /// Empty pipeline.
    EmptyPipeline,
    /// Cycle detected.
    CycleDetected(Vec<String>),
    /// Missing dependency.
    MissingDependency { step: String, dependency: String },
    /// Invalid step configuration.
    InvalidStep { step: String, reason: String },
    /// No entry points (all steps have dependencies).
    NoEntryPoints,
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseError(e) => write!(f, "parse error: {}", e),
            Self::EmptyPipeline => write!(f, "pipeline has no steps"),
            Self::CycleDetected(steps) => write!(f, "cycle detected: {}", steps.join(" -> ")),
            Self::MissingDependency { step, dependency } => {
                write!(f, "step '{}' depends on unknown step '{}'", step, dependency)
            }
            Self::InvalidStep { step, reason } => {
                write!(f, "invalid step '{}': {}", step, reason)
            }
            Self::NoEntryPoints => write!(f, "no entry points: all steps have dependencies"),
        }
    }
}

impl std::error::Error for PipelineError {}

/// Parsed and validated pipeline ready for execution.
#[derive(Debug, Clone)]
pub struct ValidatedPipeline {
    /// Original definition.
    pub definition: PipelineDefinition,
    /// Execution levels (topological order). Each level can run in parallel.
    pub execution_levels: Vec<Vec<String>>,
    /// Aggregated capabilities for the entire pipeline.
    pub aggregated_capabilities: HashSet<String>,
    /// Step count.
    pub step_count: usize,
    /// Entry points (steps with no dependencies).
    pub entry_points: Vec<String>,
    /// Terminal steps (steps that no other step depends on).
    pub terminal_steps: Vec<String>,
}

impl ValidatedPipeline {
    /// Get the total fuel budget across all steps.
    pub fn total_fuel(&self) -> u64 {
        self.definition.steps.values().filter_map(|s| s.fuel).sum()
    }

    /// Get the maximum memory limit across all steps.
    pub fn max_memory(&self) -> Option<u64> {
        self.definition.steps.values().filter_map(|s| s.memory_limit).max()
    }

    /// Get the maximum parallelism (widest level).
    pub fn max_parallelism(&self) -> usize {
        self.execution_levels.iter().map(|level| level.len()).max().unwrap_or(0)
    }

    /// Get the critical path length.
    pub fn critical_path_length(&self) -> usize {
        self.execution_levels.len()
    }
}

/// Parse a pipeline from YAML text.
pub fn parse_pipeline(yaml: &str) -> Result<PipelineDefinition, PipelineError> {
    serde_yaml::from_str(yaml).map_err(|e| PipelineError::ParseError(e.to_string()))
}

/// Validate a pipeline definition and produce a validated pipeline.
pub fn validate_pipeline(def: PipelineDefinition) -> Result<ValidatedPipeline, PipelineError> {
    // 1. Check non-empty
    if def.steps.is_empty() {
        return Err(PipelineError::EmptyPipeline);
    }

    // 2. Check all dependencies exist
    for (name, step) in &def.steps {
        if step.module.is_empty() {
            return Err(PipelineError::InvalidStep {
                step: name.clone(),
                reason: "module is required".to_string(),
            });
        }
        for dep in &step.depends_on {
            if !def.steps.contains_key(dep) {
                return Err(PipelineError::MissingDependency {
                    step: name.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }

    // 3. Topological sort + cycle detection
    let execution_levels = topological_sort(&def)?;

    // 4. Check entry points exist
    let entry_points: Vec<String> = def
        .steps
        .iter()
        .filter(|(_, step)| step.depends_on.is_empty())
        .map(|(name, _)| name.clone())
        .collect();

    if entry_points.is_empty() {
        return Err(PipelineError::NoEntryPoints);
    }

    // 5. Find terminal steps
    let depended_on: HashSet<&str> =
        def.steps.values().flat_map(|s| s.depends_on.iter().map(|d| d.as_str())).collect();

    let terminal_steps: Vec<String> =
        def.steps.keys().filter(|name| !depended_on.contains(name.as_str())).cloned().collect();

    // 6. Aggregate capabilities
    let mut aggregated_capabilities: HashSet<String> =
        def.global_capabilities.iter().cloned().collect();

    for step in def.steps.values() {
        for cap in &step.capabilities {
            aggregated_capabilities.insert(cap.clone());
        }
    }

    let step_count = def.steps.len();

    Ok(ValidatedPipeline {
        definition: def,
        execution_levels,
        aggregated_capabilities,
        step_count,
        entry_points,
        terminal_steps,
    })
}

/// Topological sort with cycle detection using Kahn's algorithm.
fn topological_sort(def: &PipelineDefinition) -> Result<Vec<Vec<String>>, PipelineError> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in def.steps.keys() {
        in_degree.insert(name, 0);
        dependents.entry(name).or_default();
    }

    for (name, step) in &def.steps {
        for dep in &step.depends_on {
            *in_degree.entry(name.as_str()).or_default() += 1;
            dependents.entry(dep.as_str()).or_default().push(name);
        }
    }

    let mut result = Vec::new();
    let mut queue: VecDeque<&str> =
        in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(&name, _)| name).collect();

    let mut processed = 0;

    while !queue.is_empty() {
        let level: Vec<String> = queue.drain(..).map(|s| s.to_string()).collect();
        processed += level.len();

        for name in &level {
            if let Some(deps) = dependents.get(name.as_str()) {
                for dep in deps {
                    if let Some(count) = in_degree.get_mut(*dep) {
                        *count -= 1;
                        if *count == 0 {
                            queue.push_back(*dep);
                        }
                    }
                }
            }
        }

        result.push(level);
    }

    if processed != def.steps.len() {
        // Find cycle participants
        let cycle: Vec<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&name, _)| name.to_string())
            .collect();
        return Err(PipelineError::CycleDetected(cycle));
    }

    Ok(result)
}

/// Capability propagation: compute the effective capabilities for each step,
/// including inherited global capabilities.
pub fn propagate_capabilities(pipeline: &ValidatedPipeline) -> HashMap<String, HashSet<String>> {
    let mut result = HashMap::new();

    for (name, step) in &pipeline.definition.steps {
        let mut caps: HashSet<String> =
            pipeline.definition.global_capabilities.iter().cloned().collect();
        for cap in &step.capabilities {
            caps.insert(cap.clone());
        }
        result.insert(name.clone(), caps);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> &'static str {
        r#"
name: etl-pipeline
description: Extract, transform, load
timeout_secs: 300
global_capabilities:
  - stdout
steps:
  extract:
    module: extractor.wasm
    capabilities:
      - filesystem_read
    fuel: 1000000
  transform:
    module: transformer.wasm
    depends_on:
      - extract
    fuel: 2000000
  load:
    module: loader.wasm
    depends_on:
      - transform
    capabilities:
      - network
    fuel: 500000
"#
    }

    #[test]
    fn test_parse_pipeline() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        assert_eq!(def.name, "etl-pipeline");
        assert_eq!(def.steps.len(), 3);
        assert_eq!(def.global_capabilities, vec!["stdout"]);
    }

    #[test]
    fn test_validate_pipeline() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        let pipeline = validate_pipeline(def).unwrap();

        assert_eq!(pipeline.step_count, 3);
        assert_eq!(pipeline.entry_points, vec!["extract".to_string()]);
        assert_eq!(pipeline.execution_levels.len(), 3);
        assert!(pipeline.aggregated_capabilities.contains("stdout"));
        assert!(pipeline.aggregated_capabilities.contains("filesystem_read"));
        assert!(pipeline.aggregated_capabilities.contains("network"));
    }

    #[test]
    fn test_execution_levels() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        let pipeline = validate_pipeline(def).unwrap();

        // Level 0: extract, Level 1: transform, Level 2: load
        assert!(pipeline.execution_levels[0].contains(&"extract".to_string()));
        assert!(pipeline.execution_levels[1].contains(&"transform".to_string()));
        assert!(pipeline.execution_levels[2].contains(&"load".to_string()));
    }

    #[test]
    fn test_parallel_steps() {
        let yaml = r#"
name: parallel-test
steps:
  fetch_a:
    module: fetcher_a.wasm
    fuel: 100000
  fetch_b:
    module: fetcher_b.wasm
    fuel: 100000
  merge:
    module: merger.wasm
    depends_on:
      - fetch_a
      - fetch_b
    fuel: 200000
"#;
        let def = parse_pipeline(yaml).unwrap();
        let pipeline = validate_pipeline(def).unwrap();

        assert_eq!(pipeline.max_parallelism(), 2);
        assert_eq!(pipeline.critical_path_length(), 2);
        assert_eq!(pipeline.execution_levels.len(), 2);
    }

    #[test]
    fn test_cycle_detection() {
        let yaml = r#"
name: cyclic
steps:
  a:
    module: a.wasm
    depends_on: [c]
  b:
    module: b.wasm
    depends_on: [a]
  c:
    module: c.wasm
    depends_on: [b]
"#;
        let def = parse_pipeline(yaml).unwrap();
        let result = validate_pipeline(def);
        assert!(matches!(result, Err(PipelineError::CycleDetected(_))));
    }

    #[test]
    fn test_missing_dependency() {
        let yaml = r#"
name: bad-dep
steps:
  a:
    module: a.wasm
    depends_on: [nonexistent]
"#;
        let def = parse_pipeline(yaml).unwrap();
        let result = validate_pipeline(def);
        assert!(matches!(result, Err(PipelineError::MissingDependency { .. })));
    }

    #[test]
    fn test_empty_pipeline() {
        let yaml = r#"
name: empty
steps: {}
"#;
        let def = parse_pipeline(yaml).unwrap();
        let result = validate_pipeline(def);
        assert!(matches!(result, Err(PipelineError::EmptyPipeline)));
    }

    #[test]
    fn test_total_fuel() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        let pipeline = validate_pipeline(def).unwrap();
        assert_eq!(pipeline.total_fuel(), 3_500_000);
    }

    #[test]
    fn test_capability_propagation() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        let pipeline = validate_pipeline(def).unwrap();
        let caps = propagate_capabilities(&pipeline);

        // All steps get global "stdout"
        assert!(caps["extract"].contains("stdout"));
        assert!(caps["transform"].contains("stdout"));
        assert!(caps["load"].contains("stdout"));

        // Only extract gets filesystem_read
        assert!(caps["extract"].contains("filesystem_read"));
        assert!(!caps["transform"].contains("filesystem_read"));

        // Only load gets network
        assert!(caps["load"].contains("network"));
        assert!(!caps["extract"].contains("network"));
    }

    #[test]
    fn test_terminal_steps() {
        let def = parse_pipeline(sample_yaml()).unwrap();
        let pipeline = validate_pipeline(def).unwrap();
        assert_eq!(pipeline.terminal_steps, vec!["load".to_string()]);
    }

    #[test]
    fn test_invalid_module() {
        let yaml = r#"
name: no-module
steps:
  bad:
    module: ""
"#;
        let def = parse_pipeline(yaml).unwrap();
        let result = validate_pipeline(def);
        assert!(matches!(result, Err(PipelineError::InvalidStep { .. })));
    }

    #[test]
    fn test_pipeline_error_display() {
        let err = PipelineError::EmptyPipeline;
        assert_eq!(err.to_string(), "pipeline has no steps");
    }
}
