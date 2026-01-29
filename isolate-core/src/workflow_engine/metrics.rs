//! Per-stage metrics collection for multi-module pipeline orchestration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::dag::Workflow;
use super::executor::{ExecutionResult, ExecutionStatus, WorkflowExecutor};
use super::nodes::NodeKind;

/// Metrics collected for a single pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageMetrics {
    pub stage_id: String,
    pub stage_name: String,
    pub start_time: Duration,
    pub end_time: Duration,
    pub duration: Duration,
    pub input_size_bytes: usize,
    pub output_size_bytes: usize,
    pub fuel_consumed: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
    pub retry_count: u32,
}

/// Aggregate metrics for an entire pipeline execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetrics {
    pub pipeline_id: String,
    pub pipeline_name: String,
    pub total_duration: Duration,
    pub total_fuel_consumed: u64,
    pub stages: Vec<StageMetrics>,
    pub stage_count: usize,
    pub succeeded_count: usize,
    pub failed_count: usize,
    pub throughput_bytes_per_sec: f64,
}

impl PipelineMetrics {
    /// Returns the slowest stage by duration, if any.
    pub fn slowest_stage(&self) -> Option<&StageMetrics> {
        self.stages.iter().max_by_key(|s| s.duration)
    }

    /// Returns the fastest stage by duration, if any.
    pub fn fastest_stage(&self) -> Option<&StageMetrics> {
        self.stages.iter().min_by_key(|s| s.duration)
    }

    /// Returns the bottleneck stage: the slowest stage that succeeded.
    /// If all stages failed, returns the slowest failed stage.
    pub fn bottleneck(&self) -> Option<&StageMetrics> {
        let succeeded: Vec<&StageMetrics> = self.stages.iter().filter(|s| s.success).collect();
        if succeeded.is_empty() {
            return self.slowest_stage();
        }
        succeeded.into_iter().max_by_key(|s| s.duration)
    }
}

/// In-progress stage being timed.
struct ActiveStage {
    stage_id: String,
    stage_name: String,
    start: Instant,
    input_size_bytes: usize,
    fuel_consumed: Option<u64>,
    retry_count: u32,
}

/// Collects metrics during pipeline execution.
pub struct MetricsCollector {
    pipeline_id: String,
    pipeline_name: String,
    pipeline_start: Instant,
    active_stages: HashMap<String, ActiveStage>,
    completed_stages: Vec<StageMetrics>,
}

impl MetricsCollector {
    /// Create a new collector for a pipeline.
    pub fn new(pipeline_name: impl Into<String>) -> Self {
        Self {
            pipeline_id: Uuid::new_v4().to_string(),
            pipeline_name: pipeline_name.into(),
            pipeline_start: Instant::now(),
            active_stages: HashMap::new(),
            completed_stages: Vec::new(),
        }
    }

    /// Start timing a stage.
    pub fn begin_stage(&mut self, stage_id: impl Into<String>, stage_name: impl Into<String>, input_size_bytes: usize) {
        let id = stage_id.into();
        self.active_stages.insert(id.clone(), ActiveStage {
            stage_id: id,
            stage_name: stage_name.into(),
            start: Instant::now(),
            input_size_bytes,
            fuel_consumed: None,
            retry_count: 0,
        });
    }

    /// Finish timing a stage and record its output.
    pub fn end_stage(&mut self, stage_id: &str, output_size_bytes: usize, success: bool, error: Option<String>) {
        if let Some(active) = self.active_stages.remove(stage_id) {
            let end = Instant::now();
            let duration = end.duration_since(active.start);
            let start_time = active.start.duration_since(self.pipeline_start);
            let end_time = end.duration_since(self.pipeline_start);

            self.completed_stages.push(StageMetrics {
                stage_id: active.stage_id,
                stage_name: active.stage_name,
                start_time,
                end_time,
                duration,
                input_size_bytes: active.input_size_bytes,
                output_size_bytes,
                fuel_consumed: active.fuel_consumed,
                success,
                error,
                retry_count: active.retry_count,
            });
        }
    }

    /// Record fuel usage for an active stage.
    pub fn record_fuel(&mut self, stage_id: &str, fuel: u64) {
        if let Some(active) = self.active_stages.get_mut(stage_id) {
            active.fuel_consumed = Some(fuel);
        }
    }

    /// Record a retry for an active stage.
    pub fn record_retry(&mut self, stage_id: &str) {
        if let Some(active) = self.active_stages.get_mut(stage_id) {
            active.retry_count += 1;
        }
    }

    /// Finalize collection and produce aggregate pipeline metrics.
    pub fn finalize(self) -> PipelineMetrics {
        let total_duration = self.pipeline_start.elapsed();
        let total_fuel_consumed: u64 = self.completed_stages.iter()
            .filter_map(|s| s.fuel_consumed)
            .sum();
        let succeeded_count = self.completed_stages.iter().filter(|s| s.success).count();
        let failed_count = self.completed_stages.iter().filter(|s| !s.success).count();
        let stage_count = self.completed_stages.len();

        let total_output_bytes: usize = self.completed_stages.iter()
            .map(|s| s.output_size_bytes)
            .sum();
        let throughput_bytes_per_sec = if total_duration.as_secs_f64() > 0.0 {
            total_output_bytes as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        PipelineMetrics {
            pipeline_id: self.pipeline_id,
            pipeline_name: self.pipeline_name,
            total_duration,
            total_fuel_consumed,
            stages: self.completed_stages,
            stage_count,
            succeeded_count,
            failed_count,
            throughput_bytes_per_sec,
        }
    }
}

/// Wraps a `WorkflowExecutor` to automatically collect per-stage metrics.
pub struct MetricsAwareExecutor {
    executor: WorkflowExecutor,
}

impl MetricsAwareExecutor {
    pub fn new() -> Self {
        Self {
            executor: WorkflowExecutor::new(),
        }
    }

    /// Execute a workflow and return both the execution result and pipeline metrics.
    pub fn execute_with_metrics(
        &self,
        workflow: &Workflow,
        input: serde_json::Value,
    ) -> (ExecutionResult, PipelineMetrics) {
        let mut collector = MetricsCollector::new(&workflow.name);
        let order = workflow.execution_order();
        let mut outputs: HashMap<String, serde_json::Value> = HashMap::new();
        let mut node_outputs = Vec::new();
        let mut nodes_executed = 0usize;
        let mut nodes_skipped = 0usize;
        let mut has_failure = false;

        for node_id in order {
            let node = match workflow.get_node(node_id) {
                Some(n) => n,
                None => continue,
            };

            let incoming = workflow.incoming(node_id);
            let node_input = if incoming.is_empty() {
                input.clone()
            } else {
                Self::merge_inputs(&incoming, &outputs)
            };

            let input_bytes = serde_json::to_string(&node_input).unwrap_or_default().len();
            let stage_name = node.label.clone().unwrap_or_else(|| node_id.clone());
            collector.begin_stage(node_id, &stage_name, input_bytes);

            // Record fuel limit for sandbox nodes.
            if let NodeKind::Sandbox { fuel_limit, .. } = &node.kind {
                collector.record_fuel(node_id, *fuel_limit);
            }

            let result = self.executor.execute_node(node_id, &node.kind, &node_input);

            if result.success {
                let output_bytes = serde_json::to_string(&result.data).unwrap_or_default().len();
                collector.end_stage(node_id, output_bytes, true, None);
                outputs.insert(node_id.clone(), result.data.clone());
                nodes_executed += 1;
            } else {
                has_failure = true;
                let mut succeeded = false;
                for _ in 0..node.retry_count {
                    collector.record_retry(node_id);
                    let retry = self.executor.execute_node(node_id, &node.kind, &node_input);
                    if retry.success {
                        let output_bytes = serde_json::to_string(&retry.data).unwrap_or_default().len();
                        collector.end_stage(node_id, output_bytes, true, None);
                        outputs.insert(node_id.clone(), retry.data.clone());
                        nodes_executed += 1;
                        node_outputs.push(retry);
                        succeeded = true;
                        break;
                    }
                }
                if !succeeded {
                    collector.end_stage(node_id, 0, false, result.error.clone());
                    nodes_skipped += 1;
                    outputs.insert(node_id.clone(), serde_json::Value::Null);
                }
            }

            node_outputs.push(result);
        }

        let final_output = order
            .last()
            .and_then(|id| outputs.get(id))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let status = if has_failure && nodes_executed == 0 {
            ExecutionStatus::Failed
        } else if has_failure {
            ExecutionStatus::PartialSuccess
        } else {
            ExecutionStatus::Completed
        };

        let exec_result = ExecutionResult {
            workflow_name: workflow.name.clone(),
            status,
            node_outputs,
            final_output,
            nodes_executed,
            nodes_skipped,
        };

        let pipeline_metrics = collector.finalize();
        (exec_result, pipeline_metrics)
    }

    fn merge_inputs(
        incoming: &[&super::dag::Edge],
        outputs: &HashMap<String, serde_json::Value>,
    ) -> serde_json::Value {
        if incoming.len() == 1 {
            return outputs
                .get(incoming[0].from.as_str())
                .cloned()
                .unwrap_or(serde_json::Value::Null);
        }
        let mut merged = serde_json::Map::new();
        for edge in incoming {
            if let Some(val) = outputs.get(edge.from.as_str()) {
                merged.insert(edge.from.as_str().to_string(), val.clone());
            }
        }
        serde_json::Value::Object(merged)
    }
}

impl Default for MetricsAwareExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_engine::dag::WorkflowBuilder;
    use crate::workflow_engine::nodes::*;

    // ── StageMetrics basic construction ──

    #[test]
    fn test_stage_metrics_fields() {
        let sm = StageMetrics {
            stage_id: "s1".into(),
            stage_name: "Stage One".into(),
            start_time: Duration::from_millis(0),
            end_time: Duration::from_millis(100),
            duration: Duration::from_millis(100),
            input_size_bytes: 256,
            output_size_bytes: 512,
            fuel_consumed: Some(5000),
            success: true,
            error: None,
            retry_count: 0,
        };
        assert_eq!(sm.stage_id, "s1");
        assert_eq!(sm.duration, Duration::from_millis(100));
        assert!(sm.success);
        assert_eq!(sm.fuel_consumed, Some(5000));
    }

    #[test]
    fn test_stage_metrics_with_error() {
        let sm = StageMetrics {
            stage_id: "s2".into(),
            stage_name: "Failing Stage".into(),
            start_time: Duration::from_millis(0),
            end_time: Duration::from_millis(50),
            duration: Duration::from_millis(50),
            input_size_bytes: 128,
            output_size_bytes: 0,
            fuel_consumed: None,
            success: false,
            error: Some("timeout".into()),
            retry_count: 2,
        };
        assert!(!sm.success);
        assert_eq!(sm.error.as_deref(), Some("timeout"));
        assert_eq!(sm.retry_count, 2);
    }

    // ── PipelineMetrics aggregation methods ──

    fn sample_pipeline_metrics() -> PipelineMetrics {
        PipelineMetrics {
            pipeline_id: "p1".into(),
            pipeline_name: "test-pipeline".into(),
            total_duration: Duration::from_millis(300),
            total_fuel_consumed: 15000,
            stages: vec![
                StageMetrics {
                    stage_id: "fast".into(),
                    stage_name: "Fast".into(),
                    start_time: Duration::ZERO,
                    end_time: Duration::from_millis(50),
                    duration: Duration::from_millis(50),
                    input_size_bytes: 100,
                    output_size_bytes: 200,
                    fuel_consumed: Some(5000),
                    success: true,
                    error: None,
                    retry_count: 0,
                },
                StageMetrics {
                    stage_id: "slow".into(),
                    stage_name: "Slow".into(),
                    start_time: Duration::from_millis(50),
                    end_time: Duration::from_millis(250),
                    duration: Duration::from_millis(200),
                    input_size_bytes: 200,
                    output_size_bytes: 400,
                    fuel_consumed: Some(10000),
                    success: true,
                    error: None,
                    retry_count: 0,
                },
                StageMetrics {
                    stage_id: "fail".into(),
                    stage_name: "Fail".into(),
                    start_time: Duration::from_millis(250),
                    end_time: Duration::from_millis(300),
                    duration: Duration::from_millis(50),
                    input_size_bytes: 400,
                    output_size_bytes: 0,
                    fuel_consumed: None,
                    success: false,
                    error: Some("crashed".into()),
                    retry_count: 1,
                },
            ],
            stage_count: 3,
            succeeded_count: 2,
            failed_count: 1,
            throughput_bytes_per_sec: 2000.0,
        }
    }

    #[test]
    fn test_slowest_stage() {
        let pm = sample_pipeline_metrics();
        let slowest = pm.slowest_stage().unwrap();
        assert_eq!(slowest.stage_id, "slow");
        assert_eq!(slowest.duration, Duration::from_millis(200));
    }

    #[test]
    fn test_fastest_stage() {
        let pm = sample_pipeline_metrics();
        let fastest = pm.fastest_stage().unwrap();
        // Both "fast" and "fail" are 50ms; min_by_key returns first match
        assert_eq!(fastest.duration, Duration::from_millis(50));
    }

    #[test]
    fn test_bottleneck_is_slowest_succeeded() {
        let pm = sample_pipeline_metrics();
        let bottleneck = pm.bottleneck().unwrap();
        assert_eq!(bottleneck.stage_id, "slow");
        assert!(bottleneck.success);
    }

    #[test]
    fn test_bottleneck_fallback_to_slowest_when_all_failed() {
        let pm = PipelineMetrics {
            pipeline_id: "p2".into(),
            pipeline_name: "all-fail".into(),
            total_duration: Duration::from_millis(100),
            total_fuel_consumed: 0,
            stages: vec![
                StageMetrics {
                    stage_id: "a".into(),
                    stage_name: "A".into(),
                    start_time: Duration::ZERO,
                    end_time: Duration::from_millis(30),
                    duration: Duration::from_millis(30),
                    input_size_bytes: 0,
                    output_size_bytes: 0,
                    fuel_consumed: None,
                    success: false,
                    error: Some("err".into()),
                    retry_count: 0,
                },
                StageMetrics {
                    stage_id: "b".into(),
                    stage_name: "B".into(),
                    start_time: Duration::from_millis(30),
                    end_time: Duration::from_millis(100),
                    duration: Duration::from_millis(70),
                    input_size_bytes: 0,
                    output_size_bytes: 0,
                    fuel_consumed: None,
                    success: false,
                    error: Some("err".into()),
                    retry_count: 0,
                },
            ],
            stage_count: 2,
            succeeded_count: 0,
            failed_count: 2,
            throughput_bytes_per_sec: 0.0,
        };
        let bottleneck = pm.bottleneck().unwrap();
        assert_eq!(bottleneck.stage_id, "b");
    }

    #[test]
    fn test_empty_pipeline_metrics() {
        let pm = PipelineMetrics {
            pipeline_id: "empty".into(),
            pipeline_name: "empty".into(),
            total_duration: Duration::ZERO,
            total_fuel_consumed: 0,
            stages: vec![],
            stage_count: 0,
            succeeded_count: 0,
            failed_count: 0,
            throughput_bytes_per_sec: 0.0,
        };
        assert!(pm.slowest_stage().is_none());
        assert!(pm.fastest_stage().is_none());
        assert!(pm.bottleneck().is_none());
    }

    // ── MetricsCollector lifecycle ──

    #[test]
    fn test_collector_basic_lifecycle() {
        let mut collector = MetricsCollector::new("test-pipeline");
        collector.begin_stage("s1", "Stage 1", 100);
        collector.end_stage("s1", 200, true, None);

        let pm = collector.finalize();
        assert_eq!(pm.pipeline_name, "test-pipeline");
        assert_eq!(pm.stage_count, 1);
        assert_eq!(pm.succeeded_count, 1);
        assert_eq!(pm.failed_count, 0);
        assert_eq!(pm.stages[0].stage_id, "s1");
        assert_eq!(pm.stages[0].input_size_bytes, 100);
        assert_eq!(pm.stages[0].output_size_bytes, 200);
        assert!(pm.stages[0].success);
    }

    #[test]
    fn test_collector_fuel_and_retry() {
        let mut collector = MetricsCollector::new("fuel-test");
        collector.begin_stage("s1", "Sandbox Stage", 50);
        collector.record_fuel("s1", 42000);
        collector.record_retry("s1");
        collector.record_retry("s1");
        collector.end_stage("s1", 60, true, None);

        let pm = collector.finalize();
        assert_eq!(pm.total_fuel_consumed, 42000);
        assert_eq!(pm.stages[0].fuel_consumed, Some(42000));
        assert_eq!(pm.stages[0].retry_count, 2);
    }

    #[test]
    fn test_collector_multiple_stages() {
        let mut collector = MetricsCollector::new("multi");
        collector.begin_stage("a", "A", 10);
        collector.end_stage("a", 20, true, None);
        collector.begin_stage("b", "B", 20);
        collector.record_fuel("b", 1000);
        collector.end_stage("b", 30, true, None);
        collector.begin_stage("c", "C", 30);
        collector.end_stage("c", 0, false, Some("error".into()));

        let pm = collector.finalize();
        assert_eq!(pm.stage_count, 3);
        assert_eq!(pm.succeeded_count, 2);
        assert_eq!(pm.failed_count, 1);
        assert_eq!(pm.total_fuel_consumed, 1000);
    }

    #[test]
    fn test_collector_end_nonexistent_stage_is_noop() {
        let mut collector = MetricsCollector::new("noop");
        collector.end_stage("nonexistent", 0, false, None);
        let pm = collector.finalize();
        assert_eq!(pm.stage_count, 0);
    }

    // ── MetricsAwareExecutor integration ──

    #[test]
    fn test_metrics_aware_executor_single_node() {
        let wf = WorkflowBuilder::new("single")
            .add_node(Node::new("n1", NodeKind::Passthrough))
            .build()
            .unwrap();

        let exec = MetricsAwareExecutor::new();
        let (result, metrics) = exec.execute_with_metrics(&wf, serde_json::json!({"x": 1}));

        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.nodes_executed, 1);
        assert_eq!(metrics.pipeline_name, "single");
        assert_eq!(metrics.stage_count, 1);
        assert_eq!(metrics.succeeded_count, 1);
        assert_eq!(metrics.failed_count, 0);
        assert!(metrics.stages[0].success);
    }

    #[test]
    fn test_metrics_aware_executor_chain() {
        let wf = WorkflowBuilder::new("chain")
            .add_node(Node::new("extract", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.name".into()),
            }))
            .add_node(Node::new("identity", NodeKind::Transform {
                transform: TransformFn::Identity,
            }))
            .add_edge("extract", "identity")
            .unwrap()
            .build()
            .unwrap();

        let exec = MetricsAwareExecutor::new();
        let (result, metrics) = exec.execute_with_metrics(&wf, serde_json::json!({"name": "test"}));

        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(metrics.stage_count, 2);
        assert_eq!(metrics.succeeded_count, 2);
        assert!(metrics.total_duration >= Duration::ZERO);
        // Verify ordering: extract before identity
        assert_eq!(metrics.stages[0].stage_id, "extract");
        assert_eq!(metrics.stages[1].stage_id, "identity");
    }

    #[test]
    fn test_metrics_aware_executor_sandbox_records_fuel() {
        let wf = WorkflowBuilder::new("sandbox-metrics")
            .add_node(Node::new("run", NodeKind::Sandbox {
                module_name: "test.wasm".into(),
                fuel_limit: 100_000,
            }))
            .build()
            .unwrap();

        let exec = MetricsAwareExecutor::new();
        let (result, metrics) = exec.execute_with_metrics(&wf, serde_json::json!({}));

        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(metrics.total_fuel_consumed, 100_000);
        assert_eq!(metrics.stages[0].fuel_consumed, Some(100_000));
    }

    #[test]
    fn test_metrics_aware_executor_diamond() {
        let wf = WorkflowBuilder::new("diamond")
            .add_node(Node::new("start", NodeKind::Passthrough))
            .add_node(Node::new("left", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.a".into()),
            }))
            .add_node(Node::new("right", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.b".into()),
            }))
            .add_node(Node::new("merge", NodeKind::FanIn {
                merge_strategy: MergeStrategy::Collect,
            }))
            .add_edge("start", "left").unwrap()
            .add_edge("start", "right").unwrap()
            .add_edge("left", "merge").unwrap()
            .add_edge("right", "merge").unwrap()
            .build()
            .unwrap();

        let exec = MetricsAwareExecutor::new();
        let (result, metrics) = exec.execute_with_metrics(&wf, serde_json::json!({"a": 1, "b": 2}));

        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(metrics.stage_count, 4);
        assert_eq!(metrics.succeeded_count, 4);
        assert!(metrics.slowest_stage().is_some());
        assert!(metrics.fastest_stage().is_some());
    }
}
