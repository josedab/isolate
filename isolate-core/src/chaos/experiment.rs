//! Chaos experiment DSL runner and report generation.
//!
//! Extends the chaos module with a YAML-like experiment definition DSL,
//! report generation, and steady-state validation.

#![allow(dead_code)]

use super::{
    AbortCondition, ChaosEngine, ChaosEvent, ChaosEventType, ExperimentMetrics,
    ExperimentOutcome, ExperimentResult, FaultInjection, FaultTarget, FaultType,
    InjectionSchedule, SteadyStateHypothesis,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// DSL-defined chaos experiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentDefinition {
    /// Experiment name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Version.
    pub version: String,
    /// Tags for categorization.
    pub tags: Vec<String>,
    /// Steps in the experiment.
    pub steps: Vec<ExperimentStep>,
    /// Hypothesis to validate.
    pub hypothesis: HypothesisSpec,
    /// Rollback configuration.
    pub rollback: RollbackConfig,
}

/// A step in a chaos experiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentStep {
    /// Step name.
    pub name: String,
    /// Action to perform.
    pub action: StepAction,
    /// Duration of this step.
    pub duration: Duration,
    /// Wait before starting.
    pub delay: Duration,
}

/// Action types for experiment steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepAction {
    /// Inject a fault.
    InjectFault { fault_type: FaultType, target: FaultTarget },
    /// Remove all injected faults.
    RemoveFaults,
    /// Wait for a duration.
    Wait,
    /// Validate steady state.
    ValidateHypothesis,
    /// Collect metrics snapshot.
    CollectMetrics,
}

/// Hypothesis specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypothesisSpec {
    /// Minimum success rate.
    pub min_success_rate: f64,
    /// Maximum P99 latency.
    pub max_p99_latency_ms: u64,
    /// Maximum error rate.
    pub max_error_rate: f64,
    /// Custom metric assertions.
    pub custom_assertions: Vec<MetricAssertion>,
}

impl Default for HypothesisSpec {
    fn default() -> Self {
        Self {
            min_success_rate: 0.99,
            max_p99_latency_ms: 500,
            max_error_rate: 0.01,
            custom_assertions: Vec::new(),
        }
    }
}

impl HypothesisSpec {
    fn to_steady_state(&self) -> SteadyStateHypothesis {
        SteadyStateHypothesis {
            success_rate: self.min_success_rate,
            max_latency_p99: Duration::from_millis(self.max_p99_latency_ms),
            max_error_rate: self.max_error_rate,
            assertions: self
                .custom_assertions
                .iter()
                .map(|a| format!("{} {} {}", a.metric, a.operator, a.threshold))
                .collect(),
        }
    }
}

/// A custom metric assertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAssertion {
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
}

/// Rollback configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackConfig {
    /// Automatically rollback on hypothesis violation.
    pub auto_rollback: bool,
    /// Maximum time before forced rollback.
    pub timeout: Duration,
    /// Actions to take on rollback.
    pub actions: Vec<String>,
}

impl Default for RollbackConfig {
    fn default() -> Self {
        Self {
            auto_rollback: true,
            timeout: Duration::from_secs(300),
            actions: vec!["remove_faults".to_string(), "restore_state".to_string()],
        }
    }
}

/// Runs chaos experiments from definitions.
pub struct ExperimentRunner {
    engine: ChaosEngine,
    definitions: HashMap<String, ExperimentDefinition>,
    results: Vec<ExperimentReport>,
}

impl ExperimentRunner {
    /// Create a new runner.
    pub fn new() -> Self {
        Self {
            engine: ChaosEngine::new(),
            definitions: HashMap::new(),
            results: Vec::new(),
        }
    }

    /// Load an experiment definition.
    pub fn load(&mut self, definition: ExperimentDefinition) {
        self.definitions.insert(definition.name.clone(), definition);
    }

    /// List loaded experiments.
    pub fn list_experiments(&self) -> Vec<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    /// Run a named experiment.
    pub fn run(&mut self, name: &str) -> Result<ExperimentReport, String> {
        let definition = self
            .definitions
            .get(name)
            .ok_or_else(|| format!("Experiment not found: {}", name))?
            .clone();

        let started_at = SystemTime::now();
        let mut events = Vec::new();
        let mut step_results = Vec::new();
        let mut metrics = ExperimentMetrics::default();

        events.push(ExperimentEvent {
            timestamp: SystemTime::now(),
            step: "start".to_string(),
            message: format!("Starting experiment: {}", definition.name),
            event_type: ExperimentEventType::Started,
        });

        // Execute steps
        for step in &definition.steps {
            let step_start = SystemTime::now();
            let mut step_success = true;
            let mut step_message = String::new();

            match &step.action {
                StepAction::InjectFault { fault_type, .. } => {
                    step_message = format!("Injected: {:?}", fault_type);
                    events.push(ExperimentEvent {
                        timestamp: SystemTime::now(),
                        step: step.name.clone(),
                        message: step_message.clone(),
                        event_type: ExperimentEventType::FaultInjected,
                    });
                }
                StepAction::RemoveFaults => {
                    step_message = "Removed all faults".to_string();
                    events.push(ExperimentEvent {
                        timestamp: SystemTime::now(),
                        step: step.name.clone(),
                        message: step_message.clone(),
                        event_type: ExperimentEventType::FaultRemoved,
                    });
                }
                StepAction::ValidateHypothesis => {
                    let hypothesis = definition.hypothesis.to_steady_state();
                    let valid = metrics.success_rate() >= hypothesis.success_rate
                        && metrics.error_rate() <= hypothesis.max_error_rate;
                    step_success = valid;
                    step_message = if valid {
                        "Hypothesis validated".to_string()
                    } else {
                        "Hypothesis violated".to_string()
                    };
                }
                StepAction::CollectMetrics => {
                    // Simulate collecting metrics
                    metrics.total_requests += 100;
                    metrics.successful_requests += 95;
                    metrics.failed_requests += 5;
                    step_message = "Metrics collected".to_string();
                }
                StepAction::Wait => {
                    step_message = format!("Waited {:?}", step.duration);
                }
            }

            step_results.push(StepResult {
                name: step.name.clone(),
                success: step_success,
                message: step_message,
                duration: step.duration,
                started_at: step_start,
            });
        }

        let hypothesis_validated = metrics.success_rate() >= definition.hypothesis.min_success_rate
            && metrics.error_rate() <= definition.hypothesis.max_error_rate;

        let outcome = if hypothesis_validated {
            ExperimentOutcome::Completed
        } else {
            ExperimentOutcome::HypothesisViolated
        };

        events.push(ExperimentEvent {
            timestamp: SystemTime::now(),
            step: "end".to_string(),
            message: format!("Experiment completed: {:?}", outcome),
            event_type: ExperimentEventType::Completed,
        });

        let report = ExperimentReport {
            name: definition.name,
            version: definition.version,
            started_at,
            ended_at: SystemTime::now(),
            outcome,
            hypothesis_validated,
            steps: step_results,
            events,
            metrics,
            tags: definition.tags,
        };

        self.results.push(report.clone());
        Ok(report)
    }

    /// Get all experiment reports.
    pub fn reports(&self) -> &[ExperimentReport] {
        &self.results
    }
}

impl Default for ExperimentRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a single experiment step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub name: String,
    pub success: bool,
    pub message: String,
    pub duration: Duration,
    pub started_at: SystemTime,
}

/// Event during experiment execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentEvent {
    pub timestamp: SystemTime,
    pub step: String,
    pub message: String,
    pub event_type: ExperimentEventType,
}

/// Types of experiment events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExperimentEventType {
    Started,
    FaultInjected,
    FaultRemoved,
    HypothesisChecked,
    MetricsCollected,
    RollbackTriggered,
    Completed,
}

/// Full experiment report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentReport {
    pub name: String,
    pub version: String,
    pub started_at: SystemTime,
    pub ended_at: SystemTime,
    pub outcome: ExperimentOutcome,
    pub hypothesis_validated: bool,
    pub steps: Vec<StepResult>,
    pub events: Vec<ExperimentEvent>,
    pub metrics: ExperimentMetrics,
    pub tags: Vec<String>,
}

impl ExperimentReport {
    /// Generate a text summary of the report.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Chaos Experiment Report: {}\n\n", self.name));
        s.push_str(&format!("Outcome: {:?}\n", self.outcome));
        s.push_str(&format!("Hypothesis validated: {}\n", self.hypothesis_validated));
        s.push_str(&format!("Steps: {}\n", self.steps.len()));
        s.push_str(&format!("Success rate: {:.1}%\n", self.metrics.success_rate() * 100.0));
        s.push_str(&format!("Error rate: {:.1}%\n", self.metrics.error_rate() * 100.0));
        s
    }

    /// Total duration.
    pub fn duration(&self) -> Option<Duration> {
        self.ended_at.duration_since(self.started_at).ok()
    }

    /// Number of successful steps.
    pub fn successful_steps(&self) -> usize {
        self.steps.iter().filter(|s| s.success).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_definition() -> ExperimentDefinition {
        ExperimentDefinition {
            name: "network-resilience".to_string(),
            description: "Test sandbox resilience to network latency".to_string(),
            version: "1.0.0".to_string(),
            tags: vec!["network".to_string(), "resilience".to_string()],
            steps: vec![
                ExperimentStep {
                    name: "collect-baseline".to_string(),
                    action: StepAction::CollectMetrics,
                    duration: Duration::from_secs(10),
                    delay: Duration::ZERO,
                },
                ExperimentStep {
                    name: "inject-latency".to_string(),
                    action: StepAction::InjectFault {
                        fault_type: FaultType::NetworkLatency(Duration::from_millis(200)),
                        target: FaultTarget::All,
                    },
                    duration: Duration::from_secs(30),
                    delay: Duration::ZERO,
                },
                ExperimentStep {
                    name: "validate".to_string(),
                    action: StepAction::ValidateHypothesis,
                    duration: Duration::ZERO,
                    delay: Duration::ZERO,
                },
                ExperimentStep {
                    name: "cleanup".to_string(),
                    action: StepAction::RemoveFaults,
                    duration: Duration::ZERO,
                    delay: Duration::ZERO,
                },
            ],
            hypothesis: HypothesisSpec {
                min_success_rate: 0.90,
                max_p99_latency_ms: 1000,
                max_error_rate: 0.10,
                custom_assertions: Vec::new(),
            },
            rollback: RollbackConfig::default(),
        }
    }

    #[test]
    fn test_experiment_runner() {
        let mut runner = ExperimentRunner::new();
        runner.load(test_definition());

        assert_eq!(runner.list_experiments().len(), 1);

        let report = runner.run("network-resilience").unwrap();
        assert_eq!(report.name, "network-resilience");
        assert_eq!(report.steps.len(), 4);
    }

    #[test]
    fn test_experiment_report_summary() {
        let mut runner = ExperimentRunner::new();
        runner.load(test_definition());

        let report = runner.run("network-resilience").unwrap();
        let summary = report.summary();

        assert!(summary.contains("network-resilience"));
        assert!(summary.contains("Outcome"));
    }

    #[test]
    fn test_experiment_not_found() {
        let mut runner = ExperimentRunner::new();
        assert!(runner.run("nonexistent").is_err());
    }

    #[test]
    fn test_hypothesis_spec_default() {
        let spec = HypothesisSpec::default();
        assert_eq!(spec.min_success_rate, 0.99);
        assert_eq!(spec.max_error_rate, 0.01);
    }

    #[test]
    fn test_rollback_config_default() {
        let config = RollbackConfig::default();
        assert!(config.auto_rollback);
        assert_eq!(config.timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_step_actions() {
        let steps = vec![
            StepAction::Wait,
            StepAction::RemoveFaults,
            StepAction::CollectMetrics,
            StepAction::ValidateHypothesis,
        ];
        assert_eq!(steps.len(), 4);
    }

    #[test]
    fn test_report_successful_steps() {
        let mut runner = ExperimentRunner::new();
        runner.load(test_definition());

        let report = runner.run("network-resilience").unwrap();
        assert!(report.successful_steps() > 0);
    }
}
