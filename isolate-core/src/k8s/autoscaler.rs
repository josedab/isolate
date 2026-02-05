//! Horizontal Pod Autoscaler (HPA) integration for Isolate sandbox pools.
//!
//! Provides custom metrics and scaling policies for Kubernetes HPA to scale
//! sandbox pools based on queue depth, warm pool utilization, and request latency.
//!
//! ```rust,ignore
//! use isolate_core::k8s::autoscaler::{
//!     HpaConfig, SandboxHpa, ScalingMetric, ScalingPolicy, MetricValue,
//! };
//!
//! let config = HpaConfig::default();
//! let mut hpa = SandboxHpa::new(config);
//!
//! hpa.record_metric(ScalingMetric::QueueDepth, 15.0);
//! let recommendation = hpa.evaluate();
//! assert!(recommendation.target_replicas >= 1);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// HPA configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpaConfig {
    /// Minimum replicas.
    pub min_replicas: u32,
    /// Maximum replicas.
    pub max_replicas: u32,
    /// Target metrics with thresholds.
    pub metrics: Vec<MetricTarget>,
    /// Scale-up stabilization window.
    pub scale_up_stabilization: Duration,
    /// Scale-down stabilization window.
    pub scale_down_stabilization: Duration,
    /// Maximum scale-up rate per evaluation (percentage).
    pub max_scale_up_percent: u32,
    /// Maximum scale-down rate per evaluation (percentage).
    pub max_scale_down_percent: u32,
}

impl Default for HpaConfig {
    fn default() -> Self {
        Self {
            min_replicas: 1,
            max_replicas: 50,
            metrics: vec![
                MetricTarget {
                    metric: ScalingMetric::QueueDepth,
                    target_value: 10.0,
                    target_type: TargetType::AverageValue,
                },
                MetricTarget {
                    metric: ScalingMetric::WarmPoolUtilization,
                    target_value: 70.0,
                    target_type: TargetType::Utilization,
                },
            ],
            scale_up_stabilization: Duration::from_secs(30),
            scale_down_stabilization: Duration::from_secs(300),
            max_scale_up_percent: 100,
            max_scale_down_percent: 10,
        }
    }
}

/// Metric used for scaling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScalingMetric {
    /// Number of pending sandbox requests.
    QueueDepth,
    /// Warm pool utilization percentage.
    WarmPoolUtilization,
    /// Average request latency in milliseconds.
    RequestLatencyMs,
    /// Active sandbox count.
    ActiveSandboxes,
    /// CPU utilization percentage.
    CpuUtilization,
    /// Memory utilization percentage.
    MemoryUtilization,
}

impl std::fmt::Display for ScalingMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueDepth => write!(f, "queue_depth"),
            Self::WarmPoolUtilization => write!(f, "warm_pool_utilization"),
            Self::RequestLatencyMs => write!(f, "request_latency_ms"),
            Self::ActiveSandboxes => write!(f, "active_sandboxes"),
            Self::CpuUtilization => write!(f, "cpu_utilization"),
            Self::MemoryUtilization => write!(f, "memory_utilization"),
        }
    }
}

/// How to interpret the target value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetType {
    /// Target as a percentage utilization.
    Utilization,
    /// Target as an absolute average value.
    AverageValue,
    /// Target as an absolute value.
    Value,
}

/// A metric target for HPA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricTarget {
    pub metric: ScalingMetric,
    pub target_value: f64,
    pub target_type: TargetType,
}

/// A recorded metric value.
#[derive(Debug, Clone)]
pub struct MetricValue {
    pub metric: ScalingMetric,
    pub value: f64,
    pub timestamp: Instant,
}

/// Scaling recommendation from the HPA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingRecommendation {
    /// Recommended target replicas.
    pub target_replicas: u32,
    /// Current replicas.
    pub current_replicas: u32,
    /// Change from current.
    pub delta: i32,
    /// Reason for the recommendation.
    pub reason: String,
    /// Individual metric recommendations.
    pub metric_recommendations: Vec<MetricRecommendation>,
}

/// Per-metric recommendation detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRecommendation {
    pub metric: String,
    pub current_value: f64,
    pub target_value: f64,
    pub recommended_replicas: u32,
}

/// Kubernetes-style status condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusCondition {
    /// Condition type (e.g., "ScalingActive", "AbleToScale", "ScalingLimited").
    pub condition_type: String,
    /// Status: "True", "False", or "Unknown".
    pub status: String,
    /// Machine-readable reason.
    pub reason: String,
    /// Human-readable message.
    pub message: String,
    /// Last transition time.
    pub last_transition: std::time::SystemTime,
}

impl StatusCondition {
    pub fn scaling_active(active: bool, reason: &str) -> Self {
        Self {
            condition_type: "ScalingActive".to_string(),
            status: if active { "True" } else { "False" }.to_string(),
            reason: reason.to_string(),
            message: if active {
                "HPA is actively scaling".to_string()
            } else {
                format!("HPA is not scaling: {}", reason)
            },
            last_transition: std::time::SystemTime::now(),
        }
    }

    pub fn able_to_scale(able: bool, reason: &str) -> Self {
        Self {
            condition_type: "AbleToScale".to_string(),
            status: if able { "True" } else { "False" }.to_string(),
            reason: reason.to_string(),
            message: if able {
                "Scaling is permitted".to_string()
            } else {
                format!("Cannot scale: {}", reason)
            },
            last_transition: std::time::SystemTime::now(),
        }
    }
}

/// Sandbox Horizontal Pod Autoscaler.
pub struct SandboxHpa {
    config: HpaConfig,
    current_replicas: u32,
    metric_values: HashMap<ScalingMetric, Vec<MetricValue>>,
    last_scale_up: Option<Instant>,
    last_scale_down: Option<Instant>,
    conditions: Vec<StatusCondition>,
}

impl SandboxHpa {
    /// Create a new HPA.
    pub fn new(config: HpaConfig) -> Self {
        let current_replicas = config.min_replicas;
        Self {
            config,
            current_replicas,
            metric_values: HashMap::new(),
            last_scale_up: None,
            last_scale_down: None,
            conditions: vec![StatusCondition::able_to_scale(true, "Initialized")],
        }
    }

    /// Record a metric value.
    pub fn record_metric(&mut self, metric: ScalingMetric, value: f64) {
        let entry = self.metric_values.entry(metric).or_default();
        entry.push(MetricValue { metric, value, timestamp: Instant::now() });

        // Keep only last 60 samples
        if entry.len() > 60 {
            entry.drain(..entry.len() - 60);
        }
    }

    /// Evaluate and produce a scaling recommendation.
    pub fn evaluate(&mut self) -> ScalingRecommendation {
        let mut max_recommended = self.current_replicas;
        let mut metric_recs = Vec::new();

        for target in &self.config.metrics {
            let current_avg = self.average_metric(&target.metric);
            if current_avg.is_none() {
                continue;
            }
            let current_avg = current_avg.unwrap();

            let ratio = current_avg / target.target_value;
            let recommended = ((self.current_replicas as f64) * ratio).ceil() as u32;
            let recommended =
                recommended.max(self.config.min_replicas).min(self.config.max_replicas);

            metric_recs.push(MetricRecommendation {
                metric: target.metric.to_string(),
                current_value: current_avg,
                target_value: target.target_value,
                recommended_replicas: recommended,
            });

            max_recommended = max_recommended.max(recommended);
        }

        // Apply rate limiting
        let target = self.apply_rate_limits(max_recommended);

        // Apply stabilization windows
        let target = self.apply_stabilization(target);

        let delta = target as i32 - self.current_replicas as i32;
        let reason = if delta > 0 {
            "Scaling up to meet demand".to_string()
        } else if delta < 0 {
            "Scaling down due to reduced demand".to_string()
        } else {
            "No scaling needed".to_string()
        };

        // Update state
        if delta > 0 {
            self.last_scale_up = Some(Instant::now());
            self.conditions.push(StatusCondition::scaling_active(true, "ScalingUp"));
        } else if delta < 0 {
            self.last_scale_down = Some(Instant::now());
            self.conditions.push(StatusCondition::scaling_active(true, "ScalingDown"));
        }

        self.current_replicas = target;

        ScalingRecommendation {
            target_replicas: target,
            current_replicas: self.current_replicas,
            delta,
            reason,
            metric_recommendations: metric_recs,
        }
    }

    /// Get the average value of a metric.
    fn average_metric(&self, metric: &ScalingMetric) -> Option<f64> {
        let values = self.metric_values.get(metric)?;
        if values.is_empty() {
            return None;
        }
        let sum: f64 = values.iter().map(|v| v.value).sum();
        Some(sum / values.len() as f64)
    }

    /// Apply rate limits to the target.
    fn apply_rate_limits(&self, target: u32) -> u32 {
        let max_increase = (self.current_replicas as f64 * self.config.max_scale_up_percent as f64
            / 100.0)
            .ceil() as u32;
        let max_decrease =
            (self.current_replicas as f64 * self.config.max_scale_down_percent as f64 / 100.0)
                .ceil() as u32;

        let max_up =
            self.current_replicas.saturating_add(max_increase.max(1)).min(self.config.max_replicas);
        let max_down =
            self.current_replicas.saturating_sub(max_decrease).max(self.config.min_replicas);

        target.max(max_down).min(max_up)
    }

    /// Apply stabilization windows.
    fn apply_stabilization(&self, target: u32) -> u32 {
        if target > self.current_replicas {
            if let Some(last) = self.last_scale_up {
                if last.elapsed() < self.config.scale_up_stabilization {
                    return self.current_replicas;
                }
            }
        }

        if target < self.current_replicas {
            if let Some(last) = self.last_scale_down {
                if last.elapsed() < self.config.scale_down_stabilization {
                    return self.current_replicas;
                }
            }
        }

        target
    }

    /// Get current replicas.
    pub fn current_replicas(&self) -> u32 {
        self.current_replicas
    }

    /// Get status conditions.
    pub fn conditions(&self) -> &[StatusCondition] {
        &self.conditions
    }

    /// Generate a custom metrics API response.
    pub fn custom_metrics(&self) -> HashMap<String, f64> {
        let mut metrics = HashMap::new();

        for (metric, values) in &self.metric_values {
            if let Some(last) = values.last() {
                metrics.insert(format!("isolate_{}", metric), last.value);
            }
        }

        metrics.insert("isolate_current_replicas".to_string(), self.current_replicas as f64);

        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hpa_creation() {
        let hpa = SandboxHpa::new(HpaConfig::default());
        assert_eq!(hpa.current_replicas(), 1);
        assert!(!hpa.conditions().is_empty());
    }

    #[test]
    fn test_record_metric() {
        let mut hpa = SandboxHpa::new(HpaConfig::default());
        hpa.record_metric(ScalingMetric::QueueDepth, 15.0);
        hpa.record_metric(ScalingMetric::QueueDepth, 20.0);

        let avg = hpa.average_metric(&ScalingMetric::QueueDepth);
        assert_eq!(avg, Some(17.5));
    }

    #[test]
    fn test_evaluate_scale_up() {
        let config = HpaConfig {
            min_replicas: 1,
            max_replicas: 10,
            scale_up_stabilization: Duration::ZERO,
            scale_down_stabilization: Duration::ZERO,
            metrics: vec![MetricTarget {
                metric: ScalingMetric::QueueDepth,
                target_value: 5.0,
                target_type: TargetType::AverageValue,
            }],
            ..Default::default()
        };
        let mut hpa = SandboxHpa::new(config);

        // High queue depth should trigger scale-up
        for _ in 0..5 {
            hpa.record_metric(ScalingMetric::QueueDepth, 20.0);
        }

        let rec = hpa.evaluate();
        assert!(rec.target_replicas >= 1);
    }

    #[test]
    fn test_evaluate_no_metrics() {
        let mut hpa = SandboxHpa::new(HpaConfig::default());
        let rec = hpa.evaluate();
        assert_eq!(rec.target_replicas, 1);
        assert_eq!(rec.delta, 0);
    }

    #[test]
    fn test_scaling_metric_display() {
        assert_eq!(ScalingMetric::QueueDepth.to_string(), "queue_depth");
        assert_eq!(ScalingMetric::WarmPoolUtilization.to_string(), "warm_pool_utilization");
        assert_eq!(ScalingMetric::RequestLatencyMs.to_string(), "request_latency_ms");
    }

    #[test]
    fn test_custom_metrics() {
        let mut hpa = SandboxHpa::new(HpaConfig::default());
        hpa.record_metric(ScalingMetric::QueueDepth, 10.0);
        hpa.record_metric(ScalingMetric::CpuUtilization, 75.0);

        let metrics = hpa.custom_metrics();
        assert!(metrics.contains_key("isolate_queue_depth"));
        assert!(metrics.contains_key("isolate_cpu_utilization"));
        assert!(metrics.contains_key("isolate_current_replicas"));
    }

    #[test]
    fn test_status_condition_scaling_active() {
        let cond = StatusCondition::scaling_active(true, "ScalingUp");
        assert_eq!(cond.condition_type, "ScalingActive");
        assert_eq!(cond.status, "True");
    }

    #[test]
    fn test_status_condition_able_to_scale() {
        let cond = StatusCondition::able_to_scale(false, "MaxReplicasReached");
        assert_eq!(cond.status, "False");
        assert!(cond.message.contains("Cannot scale"));
    }

    #[test]
    fn test_min_max_replicas() {
        let config = HpaConfig {
            min_replicas: 2,
            max_replicas: 5,
            scale_up_stabilization: Duration::ZERO,
            scale_down_stabilization: Duration::ZERO,
            ..Default::default()
        };
        let hpa = SandboxHpa::new(config);
        assert_eq!(hpa.current_replicas(), 2);
    }

    #[test]
    fn test_hpa_config_default() {
        let config = HpaConfig::default();
        assert_eq!(config.min_replicas, 1);
        assert_eq!(config.max_replicas, 50);
        assert_eq!(config.metrics.len(), 2);
    }

    #[test]
    fn test_metric_window_capping() {
        let mut hpa = SandboxHpa::new(HpaConfig::default());
        for i in 0..100 {
            hpa.record_metric(ScalingMetric::QueueDepth, i as f64);
        }
        // Should keep only last 60 samples
        assert_eq!(hpa.metric_values.get(&ScalingMetric::QueueDepth).unwrap().len(), 60);
    }
}
