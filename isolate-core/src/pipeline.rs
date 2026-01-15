//! Multi-sandbox pipeline orchestration with DAG execution.
//!
//! Enables chaining multiple sandboxes in a directed acyclic graph (DAG),
//! where outputs from one stage flow as inputs to the next.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::pipeline::{Pipeline, Stage, StageId};
//!
//! let pipeline = Pipeline::builder()
//!     .stage(Stage::new("transform", transform_config))
//!     .stage(Stage::new("validate", validate_config))
//!     .stage(Stage::new("output", output_config))
//!     .chain("transform", "validate")  // transform -> validate
//!     .chain("validate", "output")     // validate -> output
//!     .build()?;
//!
//! let result = pipeline.execute(input).await?;
//! ```

use crate::config::SandboxConfig;
use crate::error::{Error, Result};
use crate::sandbox::Output;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// Unique identifier for a pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StageId(pub String);

impl StageId {
    /// Create a new stage ID.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for StageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A stage in a pipeline that wraps a sandbox configuration.
#[derive(Debug, Clone)]
pub struct Stage {
    /// Unique stage identifier.
    pub id: StageId,
    /// Sandbox configuration for this stage.
    pub config: SandboxConfig,
    /// Retry policy for this stage.
    pub retry: RetryPolicy,
    /// Timeout override for this specific stage.
    pub timeout: Option<Duration>,
}

impl Stage {
    /// Create a new pipeline stage.
    pub fn new(id: impl Into<String>, config: SandboxConfig) -> Self {
        Self {
            id: StageId::new(id),
            config,
            retry: RetryPolicy::default(),
            timeout: None,
        }
    }

    /// Set retry policy.
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Set stage timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// Retry policy for a pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retries.
    pub max_retries: u32,
    /// Base delay between retries (exponential backoff applied).
    pub base_delay: Duration,
    /// Only retry on these exit codes. Empty means retry on any non-zero exit.
    pub retry_on_exit_codes: Vec<i32>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::from_millis(100),
            retry_on_exit_codes: Vec::new(),
        }
    }
}

/// How to pass data between pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataFlow {
    /// Pass stdout of upstream as stdin of downstream.
    StdoutToStdin,
    /// Discard upstream output, downstream gets original pipeline input.
    PassThrough,
}

/// Pipeline definition - a DAG of sandbox stages.
#[derive(Debug, Clone)]
pub struct PipelineDefinition {
    /// All stages in the pipeline.
    pub stages: HashMap<StageId, Stage>,
    /// Edges between stages (from -> [to]).
    edges: HashMap<StageId, Vec<(StageId, DataFlow)>>,
    /// Reverse edges (to -> [from]).
    reverse_edges: HashMap<StageId, Vec<StageId>>,
}

impl PipelineDefinition {
    /// Create a new pipeline builder.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Get stages with no incoming edges (entry points).
    pub fn entry_stages(&self) -> Vec<&StageId> {
        self.stages
            .keys()
            .filter(|id| self.reverse_edges.get(*id).map_or(true, |v| v.is_empty()))
            .collect()
    }

    /// Get stages with no outgoing edges (exit points).
    pub fn exit_stages(&self) -> Vec<&StageId> {
        self.stages
            .keys()
            .filter(|id| self.edges.get(*id).map_or(true, |v| v.is_empty()))
            .collect()
    }

    /// Get downstream stages for a given stage.
    pub fn downstream(&self, stage_id: &StageId) -> Vec<(&StageId, DataFlow)> {
        self.edges
            .get(stage_id)
            .map(|edges| edges.iter().map(|(id, df)| (id, *df)).collect())
            .unwrap_or_default()
    }

    /// Get upstream stages for a given stage.
    pub fn upstream(&self, stage_id: &StageId) -> Vec<&StageId> {
        self.reverse_edges.get(stage_id).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// Compute topological order for execution.
    pub fn topological_order(&self) -> Result<Vec<StageId>> {
        let mut in_degree: HashMap<&StageId, usize> = HashMap::new();
        for id in self.stages.keys() {
            in_degree.insert(id, 0);
        }
        for edges in self.edges.values() {
            for (to, _) in edges {
                *in_degree.entry(to).or_default() += 1;
            }
        }

        let mut queue: VecDeque<&StageId> =
            in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(id, _)| *id).collect();

        let mut order = Vec::new();
        while let Some(id) = queue.pop_front() {
            order.push(id.clone());
            if let Some(edges) = self.edges.get(id) {
                for (to, _) in edges {
                    if let Some(deg) = in_degree.get_mut(to) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(to);
                        }
                    }
                }
            }
        }

        if order.len() != self.stages.len() {
            return Err(Error::InvalidConfig("Pipeline contains a cycle".to_string()));
        }

        Ok(order)
    }

    /// Validate the pipeline definition.
    pub fn validate(&self) -> Result<()> {
        if self.stages.is_empty() {
            return Err(Error::InvalidConfig("Pipeline has no stages".to_string()));
        }

        // Check for cycles via topological sort
        self.topological_order()?;

        // Check that all edge references exist
        for (from, edges) in &self.edges {
            if !self.stages.contains_key(from) {
                return Err(Error::InvalidConfig(format!("Stage '{}' not found", from)));
            }
            for (to, _) in edges {
                if !self.stages.contains_key(to) {
                    return Err(Error::InvalidConfig(format!("Stage '{}' not found", to)));
                }
            }
        }

        Ok(())
    }

    /// Get the total number of stages.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Execute the pipeline with the given input bytes.
    ///
    /// Runs stages in topological order. For each stage, creates a sandbox,
    /// runs it with the appropriate input (based on DataFlow), and collects
    /// the output. Supports retry policies with exponential backoff.
    pub async fn execute(&self, engine: std::sync::Arc<crate::engine::WasmEngine>, input: &[u8]) -> Result<PipelineResult> {
        use crate::sandbox::Sandbox;
        use std::time::Instant;

        let pipeline_start = Instant::now();
        let order = self.topological_order()?;

        let mut stage_outputs: HashMap<StageId, Output> = HashMap::new();
        let mut stage_results = Vec::new();
        let mut failed_stage = None;

        for stage_id in &order {
            let stage = self.stages.get(stage_id).ok_or_else(|| {
                Error::InvalidConfig(format!("Stage '{}' not found", stage_id))
            })?;

            // Determine input for this stage
            let stage_input = self.resolve_stage_input(stage_id, input, &stage_outputs);

            let stage_start = Instant::now();
            let mut retries = 0u32;

            let output = loop {
                let mut sandbox = Sandbox::create_with_engine(
                    stage.config.clone(),
                    engine.clone(),
                ).await?;

                let result = sandbox.run(&stage_input).await;

                match result {
                    Ok(output) => {
                        let success = output.exit_code == 0;
                        if success || retries >= stage.retry.max_retries {
                            break output;
                        }
                    }
                    Err(e) => {
                        if retries >= stage.retry.max_retries {
                            return Err(e);
                        }
                    }
                }

                retries += 1;
                let delay = stage.retry.base_delay * 2u32.saturating_pow(retries - 1);
                tokio::time::sleep(delay).await;
            };

            let duration = stage_start.elapsed();
            let success = output.exit_code == 0;

            stage_results.push(StageResult {
                stage_id: stage_id.clone(),
                output: output.clone(),
                duration,
                retries,
            });

            if !success && failed_stage.is_none() {
                failed_stage = Some(stage_id.clone());
                // Stop pipeline on first failure
                break;
            }

            stage_outputs.insert(stage_id.clone(), output);
        }

        let success = failed_stage.is_none();
        Ok(PipelineResult {
            stage_results,
            total_duration: pipeline_start.elapsed(),
            success,
            failed_stage,
        })
    }

    /// Resolve input bytes for a stage based on upstream outputs and data flow.
    fn resolve_stage_input(
        &self,
        stage_id: &StageId,
        pipeline_input: &[u8],
        stage_outputs: &HashMap<StageId, Output>,
    ) -> Vec<u8> {
        let upstream = self.upstream(stage_id);
        if upstream.is_empty() {
            return pipeline_input.to_vec();
        }

        // Collect input from upstream stages based on data flow
        for up_id in &upstream {
            if let Some(edges) = self.edges.get(up_id) {
                for (to, flow) in edges {
                    if to == stage_id {
                        match flow {
                            DataFlow::StdoutToStdin => {
                                if let Some(output) = stage_outputs.get(up_id) {
                                    return output.stdout.clone();
                                }
                            }
                            DataFlow::PassThrough => {
                                return pipeline_input.to_vec();
                            }
                        }
                    }
                }
            }
        }

        pipeline_input.to_vec()
    }
}

/// Builder for pipeline definitions.
#[derive(Debug, Default)]
pub struct PipelineBuilder {
    stages: HashMap<StageId, Stage>,
    edges: Vec<(StageId, StageId, DataFlow)>,
}

impl PipelineBuilder {
    /// Create a new pipeline builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a stage to the pipeline.
    pub fn stage(mut self, stage: Stage) -> Self {
        self.stages.insert(stage.id.clone(), stage);
        self
    }

    /// Add a chain (edge) between two stages with stdout-to-stdin data flow.
    pub fn chain(self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.edge(from, to, DataFlow::StdoutToStdin)
    }

    /// Add an edge with specified data flow.
    pub fn edge(
        mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        data_flow: DataFlow,
    ) -> Self {
        self.edges.push((StageId::new(from), StageId::new(to), data_flow));
        self
    }

    /// Build the pipeline definition.
    pub fn build(self) -> Result<PipelineDefinition> {
        let mut edges: HashMap<StageId, Vec<(StageId, DataFlow)>> = HashMap::new();
        let mut reverse_edges: HashMap<StageId, Vec<StageId>> = HashMap::new();

        for (from, to, data_flow) in self.edges {
            edges.entry(from.clone()).or_default().push((to.clone(), data_flow));
            reverse_edges.entry(to).or_default().push(from);
        }

        let pipeline = PipelineDefinition { stages: self.stages, edges, reverse_edges };
        pipeline.validate()?;
        Ok(pipeline)
    }
}

/// Result of a pipeline stage execution.
#[derive(Debug, Clone)]
pub struct StageResult {
    /// Stage identifier.
    pub stage_id: StageId,
    /// Sandbox output.
    pub output: Output,
    /// Duration of this stage.
    pub duration: Duration,
    /// Number of retry attempts (0 = first try succeeded).
    pub retries: u32,
}

/// Result of a full pipeline execution.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Results from each stage, in execution order.
    pub stage_results: Vec<StageResult>,
    /// Total pipeline duration.
    pub total_duration: Duration,
    /// Whether all stages succeeded (exit code 0).
    pub success: bool,
    /// The first failed stage, if any.
    pub failed_stage: Option<StageId>,
}

impl PipelineResult {
    /// Get the output from the final exit stage(s).
    pub fn final_outputs(&self) -> Vec<&Output> {
        // Last stage results are typically the exit stages
        self.stage_results.last().map(|r| &r.output).into_iter().collect()
    }

    /// Get the result for a specific stage.
    pub fn stage_result(&self, stage_id: &StageId) -> Option<&StageResult> {
        self.stage_results.iter().find(|r| r.stage_id == *stage_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    fn test_config() -> SandboxConfig {
        SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::stdout())
            .build()
            .unwrap()
    }

    #[test]
    fn test_pipeline_builder_simple() {
        let pipeline = PipelineDefinition::builder()
            .stage(Stage::new("a", test_config()))
            .stage(Stage::new("b", test_config()))
            .chain("a", "b")
            .build()
            .unwrap();

        assert_eq!(pipeline.stage_count(), 2);
        assert_eq!(pipeline.entry_stages().len(), 1);
        assert_eq!(pipeline.exit_stages().len(), 1);
    }

    #[test]
    fn test_pipeline_topological_order() {
        let pipeline = PipelineDefinition::builder()
            .stage(Stage::new("a", test_config()))
            .stage(Stage::new("b", test_config()))
            .stage(Stage::new("c", test_config()))
            .chain("a", "b")
            .chain("b", "c")
            .build()
            .unwrap();

        let order = pipeline.topological_order().unwrap();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], StageId::new("a"));
        assert_eq!(order[2], StageId::new("c"));
    }

    #[test]
    fn test_pipeline_parallel_stages() {
        let pipeline = PipelineDefinition::builder()
            .stage(Stage::new("input", test_config()))
            .stage(Stage::new("process_a", test_config()))
            .stage(Stage::new("process_b", test_config()))
            .stage(Stage::new("merge", test_config()))
            .chain("input", "process_a")
            .chain("input", "process_b")
            .chain("process_a", "merge")
            .chain("process_b", "merge")
            .build()
            .unwrap();

        assert_eq!(pipeline.stage_count(), 4);
        let entries = pipeline.entry_stages();
        assert_eq!(entries.len(), 1);
        assert_eq!(*entries[0], StageId::new("input"));

        let downstream = pipeline.downstream(&StageId::new("input"));
        assert_eq!(downstream.len(), 2);
    }

    #[test]
    fn test_pipeline_cycle_detection() {
        let result = PipelineDefinition::builder()
            .stage(Stage::new("a", test_config()))
            .stage(Stage::new("b", test_config()))
            .chain("a", "b")
            .chain("b", "a")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_pipeline_empty() {
        let result = PipelineDefinition::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_stage_with_retry() {
        let stage = Stage::new("retry_stage", test_config())
            .with_retry(RetryPolicy { max_retries: 3, ..Default::default() })
            .with_timeout(Duration::from_secs(10));

        assert_eq!(stage.retry.max_retries, 3);
        assert_eq!(stage.timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_data_flow_modes() {
        let pipeline = PipelineDefinition::builder()
            .stage(Stage::new("a", test_config()))
            .stage(Stage::new("b", test_config()))
            .edge("a", "b", DataFlow::PassThrough)
            .build()
            .unwrap();

        let downstream = pipeline.downstream(&StageId::new("a"));
        assert_eq!(downstream[0].1, DataFlow::PassThrough);
    }

    #[test]
    fn test_single_stage_pipeline() {
        let pipeline = PipelineDefinition::builder()
            .stage(Stage::new("only", test_config()))
            .build()
            .unwrap();

        assert_eq!(pipeline.stage_count(), 1);
        assert_eq!(pipeline.entry_stages().len(), 1);
        assert_eq!(pipeline.exit_stages().len(), 1);
    }
}
