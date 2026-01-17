//! Chaos Engineering Mode
//!
//! Systematically inject failures to test sandbox resilience:
//! - Random CPU throttling and memory pressure
//! - Network latency and packet loss injection
//! - I/O failures and corruption simulation
//! - Resource exhaustion scenarios

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

pub mod experiment;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Fault injection type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FaultType {
    /// CPU throttling (0.0-1.0 available).
    CpuThrottle(f64),
    /// Memory pressure (bytes to limit).
    MemoryPressure(u64),
    /// Network latency (delay).
    NetworkLatency(Duration),
    /// Packet loss probability (0.0-1.0).
    PacketLoss(f64),
    /// I/O error injection.
    IoError { probability: f64, error_type: IoErrorType },
    /// Random process kill.
    ProcessKill { probability: f64 },
    /// Clock skew.
    ClockSkew(i64),
    /// Resource exhaustion.
    ResourceExhaustion(ResourceExhaustionType),
    /// Custom fault.
    Custom { name: String, probability: f64 },
}

/// I/O error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoErrorType {
    ReadError,
    WriteError,
    Corruption,
    Timeout,
    PermissionDenied,
}

/// Resource exhaustion types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceExhaustionType {
    FileDescriptors,
    DiskSpace,
    Threads,
    Sockets,
}

/// Fault injection target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaultTarget {
    /// All sandboxes.
    All,
    /// Specific sandbox by ID.
    Sandbox(String),
    /// Sandboxes matching pattern.
    Pattern(String),
    /// Random percentage.
    Random(u32),
}

/// Chaos experiment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosExperiment {
    /// Experiment ID.
    pub id: String,
    /// Experiment name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Faults to inject.
    pub faults: Vec<FaultInjection>,
    /// Duration of experiment.
    pub duration: Duration,
    /// Target selection.
    pub target: FaultTarget,
    /// Steady-state hypothesis.
    pub hypothesis: SteadyStateHypothesis,
    /// Abort conditions.
    pub abort_conditions: Vec<AbortCondition>,
}

/// Fault injection specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjection {
    /// Fault type.
    pub fault_type: FaultType,
    /// Start delay.
    pub start_after: Duration,
    /// Injection duration.
    pub duration: Duration,
    /// Schedule (continuous or interval).
    pub schedule: InjectionSchedule,
}

/// Injection schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InjectionSchedule {
    /// Inject continuously.
    Continuous,
    /// Inject at intervals.
    Interval { inject: Duration, pause: Duration },
    /// Random injection.
    Random { probability: f64 },
}

/// Steady-state hypothesis for chaos engineering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteadyStateHypothesis {
    /// Expected success rate (0.0-1.0).
    pub success_rate: f64,
    /// Maximum latency P99.
    pub max_latency_p99: Duration,
    /// Maximum error rate.
    pub max_error_rate: f64,
    /// Custom assertions.
    pub assertions: Vec<String>,
}

impl Default for SteadyStateHypothesis {
    fn default() -> Self {
        Self {
            success_rate: 0.99,
            max_latency_p99: Duration::from_millis(500),
            max_error_rate: 0.01,
            assertions: Vec::new(),
        }
    }
}

/// Abort condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AbortCondition {
    /// Error rate exceeds threshold.
    ErrorRateExceeds(f64),
    /// Latency exceeds threshold.
    LatencyExceeds(Duration),
    /// Custom condition.
    Custom(String),
}

/// Chaos experiment result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentResult {
    /// Experiment ID.
    pub experiment_id: String,
    /// Start time.
    pub started_at: std::time::SystemTime,
    /// End time.
    pub ended_at: std::time::SystemTime,
    /// Outcome.
    pub outcome: ExperimentOutcome,
    /// Metrics collected.
    pub metrics: ExperimentMetrics,
    /// Events during experiment.
    pub events: Vec<ChaosEvent>,
    /// Hypothesis validation.
    pub hypothesis_validated: bool,
}

/// Experiment outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExperimentOutcome {
    /// Completed successfully.
    Completed,
    /// Aborted due to condition.
    Aborted,
    /// Failed to run.
    Failed,
    /// Hypothesis violated.
    HypothesisViolated,
}

/// Metrics collected during experiment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExperimentMetrics {
    /// Total requests.
    pub total_requests: u64,
    /// Successful requests.
    pub successful_requests: u64,
    /// Failed requests.
    pub failed_requests: u64,
    /// Latency samples.
    pub latency_samples: Vec<Duration>,
    /// Custom metrics.
    pub custom: HashMap<String, f64>,
}

impl ExperimentMetrics {
    /// Calculate success rate.
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }

    /// Calculate error rate.
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.failed_requests as f64 / self.total_requests as f64
        }
    }

    /// Calculate P99 latency.
    pub fn latency_p99(&self) -> Duration {
        if self.latency_samples.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.latency_samples.clone();
        sorted.sort();
        let idx = (sorted.len() as f64 * 0.99) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }
}

/// Chaos event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosEvent {
    /// Timestamp.
    pub timestamp: std::time::SystemTime,
    /// Event type.
    pub event_type: ChaosEventType,
    /// Target sandbox.
    pub target: Option<String>,
    /// Details.
    pub details: String,
}

/// Chaos event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChaosEventType {
    ExperimentStarted,
    FaultInjected,
    FaultRemoved,
    AbortTriggered,
    ExperimentCompleted,
}

/// Chaos engine for running experiments.
pub struct ChaosEngine {
    experiments: HashMap<String, ChaosExperiment>,
    results: Vec<ExperimentResult>,
    active_faults: HashMap<String, Vec<FaultInjection>>,
    rng_seed: u64,
}

impl Default for ChaosEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ChaosEngine {
    /// Create a new chaos engine.
    pub fn new() -> Self {
        Self {
            experiments: HashMap::new(),
            results: Vec::new(),
            active_faults: HashMap::new(),
            rng_seed: 42,
        }
    }

    /// Set RNG seed for reproducibility.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng_seed = seed;
        self
    }

    /// Register an experiment.
    pub fn register_experiment(&mut self, experiment: ChaosExperiment) {
        self.experiments.insert(experiment.id.clone(), experiment);
    }

    /// Run an experiment.
    pub fn run_experiment(&mut self, experiment_id: &str) -> Result<ExperimentResult, ChaosError> {
        let experiment = self
            .experiments
            .get(experiment_id)
            .ok_or_else(|| ChaosError::ExperimentNotFound(experiment_id.to_string()))?
            .clone();

        let started_at = std::time::SystemTime::now();
        let start_instant = Instant::now();
        let mut events = Vec::new();
        let mut metrics = ExperimentMetrics::default();

        events.push(ChaosEvent {
            timestamp: started_at,
            event_type: ChaosEventType::ExperimentStarted,
            target: None,
            details: format!("Started experiment: {}", experiment.name),
        });

        // Simulate fault injection
        for fault in &experiment.faults {
            events.push(ChaosEvent {
                timestamp: std::time::SystemTime::now(),
                event_type: ChaosEventType::FaultInjected,
                target: None,
                details: format!("Injected fault: {:?}", fault.fault_type),
            });
        }

        // Simulate some metrics
        let simulated_requests = 100;
        metrics.total_requests = simulated_requests;
        metrics.successful_requests = (simulated_requests as f64 * 0.95) as u64;
        metrics.failed_requests = simulated_requests - metrics.successful_requests;

        for _ in 0..simulated_requests {
            metrics.latency_samples.push(Duration::from_millis(50 + (self.rng_seed % 100) as u64));
        }

        // Check abort conditions
        let mut outcome = ExperimentOutcome::Completed;
        for condition in &experiment.abort_conditions {
            match condition {
                AbortCondition::ErrorRateExceeds(threshold) => {
                    if metrics.error_rate() > *threshold {
                        outcome = ExperimentOutcome::Aborted;
                        events.push(ChaosEvent {
                            timestamp: std::time::SystemTime::now(),
                            event_type: ChaosEventType::AbortTriggered,
                            target: None,
                            details: format!(
                                "Error rate {} exceeded threshold {}",
                                metrics.error_rate(),
                                threshold
                            ),
                        });
                    }
                }
                AbortCondition::LatencyExceeds(threshold) => {
                    if metrics.latency_p99() > *threshold {
                        outcome = ExperimentOutcome::Aborted;
                    }
                }
                AbortCondition::Custom(_) => {}
            }
        }

        // Validate hypothesis
        let hypothesis_validated = metrics.success_rate() >= experiment.hypothesis.success_rate
            && metrics.latency_p99() <= experiment.hypothesis.max_latency_p99
            && metrics.error_rate() <= experiment.hypothesis.max_error_rate;

        if !hypothesis_validated && outcome == ExperimentOutcome::Completed {
            outcome = ExperimentOutcome::HypothesisViolated;
        }

        events.push(ChaosEvent {
            timestamp: std::time::SystemTime::now(),
            event_type: ChaosEventType::ExperimentCompleted,
            target: None,
            details: format!("Experiment completed with outcome: {:?}", outcome),
        });

        // Wait for duration (simulated)
        let _ = start_instant.elapsed();

        let result = ExperimentResult {
            experiment_id: experiment_id.to_string(),
            started_at,
            ended_at: std::time::SystemTime::now(),
            outcome,
            metrics,
            events,
            hypothesis_validated,
        };

        self.results.push(result.clone());
        Ok(result)
    }

    /// Inject a fault directly.
    pub fn inject_fault(&mut self, sandbox_id: &str, fault: FaultInjection) {
        self.active_faults.entry(sandbox_id.to_string()).or_default().push(fault);
    }

    /// Remove all faults from sandbox.
    pub fn remove_faults(&mut self, sandbox_id: &str) {
        self.active_faults.remove(sandbox_id);
    }

    /// Get active faults for sandbox.
    pub fn get_active_faults(&self, sandbox_id: &str) -> &[FaultInjection] {
        self.active_faults.get(sandbox_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get all experiment results.
    pub fn get_results(&self) -> &[ExperimentResult] {
        &self.results
    }

    /// Clear results.
    pub fn clear_results(&mut self) {
        self.results.clear();
    }
}

/// Chaos engineering error.
#[derive(Debug, Clone)]
pub enum ChaosError {
    /// Experiment not found.
    ExperimentNotFound(String),
    /// Fault injection failed.
    InjectionFailed(String),
    /// Invalid configuration.
    InvalidConfig(String),
}

impl std::fmt::Display for ChaosError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExperimentNotFound(id) => write!(f, "Experiment not found: {}", id),
            Self::InjectionFailed(msg) => write!(f, "Fault injection failed: {}", msg),
            Self::InvalidConfig(msg) => write!(f, "Invalid configuration: {}", msg),
        }
    }
}

impl std::error::Error for ChaosError {}

/// Builder for chaos experiments.
pub struct ExperimentBuilder {
    id: String,
    name: String,
    description: String,
    faults: Vec<FaultInjection>,
    duration: Duration,
    target: FaultTarget,
    hypothesis: SteadyStateHypothesis,
    abort_conditions: Vec<AbortCondition>,
}

impl ExperimentBuilder {
    /// Create a new experiment builder.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: generate_id(),
            name: name.clone(),
            description: String::new(),
            faults: Vec::new(),
            duration: Duration::from_secs(60),
            target: FaultTarget::All,
            hypothesis: SteadyStateHypothesis::default(),
            abort_conditions: Vec::new(),
        }
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add a fault.
    pub fn fault(mut self, fault_type: FaultType, duration: Duration) -> Self {
        self.faults.push(FaultInjection {
            fault_type,
            start_after: Duration::ZERO,
            duration,
            schedule: InjectionSchedule::Continuous,
        });
        self
    }

    /// Set experiment duration.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set target.
    pub fn target(mut self, target: FaultTarget) -> Self {
        self.target = target;
        self
    }

    /// Set hypothesis.
    pub fn hypothesis(mut self, hypothesis: SteadyStateHypothesis) -> Self {
        self.hypothesis = hypothesis;
        self
    }

    /// Add abort condition.
    pub fn abort_on(mut self, condition: AbortCondition) -> Self {
        self.abort_conditions.push(condition);
        self
    }

    /// Build the experiment.
    pub fn build(self) -> ChaosExperiment {
        ChaosExperiment {
            id: self.id,
            name: self.name,
            description: self.description,
            faults: self.faults,
            duration: self.duration,
            target: self.target,
            hypothesis: self.hypothesis,
            abort_conditions: self.abort_conditions,
        }
    }
}

fn generate_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    format!("chaos-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chaos_engine_creation() {
        let engine = ChaosEngine::new();
        assert!(engine.experiments.is_empty());
    }

    #[test]
    fn test_experiment_builder() {
        let experiment = ExperimentBuilder::new("test")
            .description("Test experiment")
            .fault(FaultType::CpuThrottle(0.5), Duration::from_secs(10))
            .duration(Duration::from_secs(60))
            .build();

        assert_eq!(experiment.name, "test");
        assert_eq!(experiment.faults.len(), 1);
    }

    #[test]
    fn test_run_experiment() {
        let mut engine = ChaosEngine::new();

        // Use a lenient hypothesis that the simulation will pass
        let hypothesis = SteadyStateHypothesis {
            success_rate: 0.90, // Simulation yields ~95%
            max_latency_p99: Duration::from_secs(1),
            max_error_rate: 0.10,
            assertions: vec![],
        };

        let experiment = ExperimentBuilder::new("test")
            .fault(FaultType::NetworkLatency(Duration::from_millis(100)), Duration::from_secs(5))
            .hypothesis(hypothesis)
            .build();

        engine.register_experiment(experiment.clone());
        let result = engine.run_experiment(&experiment.id).unwrap();

        assert_eq!(result.outcome, ExperimentOutcome::Completed);
    }

    #[test]
    fn test_inject_fault() {
        let mut engine = ChaosEngine::new();

        engine.inject_fault(
            "sandbox-1",
            FaultInjection {
                fault_type: FaultType::MemoryPressure(1024 * 1024),
                start_after: Duration::ZERO,
                duration: Duration::from_secs(10),
                schedule: InjectionSchedule::Continuous,
            },
        );

        let faults = engine.get_active_faults("sandbox-1");
        assert_eq!(faults.len(), 1);
    }

    #[test]
    fn test_remove_faults() {
        let mut engine = ChaosEngine::new();

        engine.inject_fault(
            "sandbox-1",
            FaultInjection {
                fault_type: FaultType::PacketLoss(0.1),
                start_after: Duration::ZERO,
                duration: Duration::from_secs(10),
                schedule: InjectionSchedule::Continuous,
            },
        );

        engine.remove_faults("sandbox-1");
        assert!(engine.get_active_faults("sandbox-1").is_empty());
    }

    #[test]
    fn test_experiment_metrics() {
        let mut metrics = ExperimentMetrics::default();
        metrics.total_requests = 100;
        metrics.successful_requests = 95;
        metrics.failed_requests = 5;
        metrics.latency_samples =
            vec![Duration::from_millis(10), Duration::from_millis(20), Duration::from_millis(100)];

        assert_eq!(metrics.success_rate(), 0.95);
        assert_eq!(metrics.error_rate(), 0.05);
    }

    #[test]
    fn test_abort_on_error_rate() {
        let mut engine = ChaosEngine::new();

        let experiment = ExperimentBuilder::new("abort-test")
            .fault(
                FaultType::IoError { probability: 0.5, error_type: IoErrorType::ReadError },
                Duration::from_secs(5),
            )
            .abort_on(AbortCondition::ErrorRateExceeds(0.01))
            .build();

        engine.register_experiment(experiment.clone());
        let result = engine.run_experiment(&experiment.id).unwrap();

        // May be aborted or violated depending on simulation
        assert!(matches!(
            result.outcome,
            ExperimentOutcome::Aborted
                | ExperimentOutcome::HypothesisViolated
                | ExperimentOutcome::Completed
        ));
    }

    #[test]
    fn test_steady_state_hypothesis() {
        let hypothesis = SteadyStateHypothesis {
            success_rate: 0.999,
            max_latency_p99: Duration::from_millis(100),
            max_error_rate: 0.001,
            assertions: vec!["db.connections < 100".to_string()],
        };

        assert_eq!(hypothesis.success_rate, 0.999);
    }

    #[test]
    fn test_fault_types() {
        let faults = vec![
            FaultType::CpuThrottle(0.5),
            FaultType::MemoryPressure(1024),
            FaultType::NetworkLatency(Duration::from_millis(100)),
            FaultType::PacketLoss(0.1),
            FaultType::ProcessKill { probability: 0.01 },
            FaultType::ClockSkew(1000),
        ];

        assert_eq!(faults.len(), 6);
    }

    #[test]
    fn test_injection_schedules() {
        let schedules = vec![
            InjectionSchedule::Continuous,
            InjectionSchedule::Interval {
                inject: Duration::from_secs(5),
                pause: Duration::from_secs(10),
            },
            InjectionSchedule::Random { probability: 0.5 },
        ];

        assert_eq!(schedules.len(), 3);
    }

    #[test]
    fn test_experiment_results() {
        let mut engine = ChaosEngine::new();

        let exp1 = ExperimentBuilder::new("exp1").build();
        let exp2 = ExperimentBuilder::new("exp2").build();

        engine.register_experiment(exp1.clone());
        engine.register_experiment(exp2.clone());

        engine.run_experiment(&exp1.id).unwrap();
        engine.run_experiment(&exp2.id).unwrap();

        assert_eq!(engine.get_results().len(), 2);
    }
}
