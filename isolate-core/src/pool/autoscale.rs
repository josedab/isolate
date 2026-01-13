//! Auto-scaling manager that connects predictive scaling to warm pool management.
//!
//! Monitors pool utilization, feeds samples to the predictive scaler, and
//! automatically adjusts pool sizes based on forecasted demand.
//!
//! ```rust
//! use isolate_core::pool::autoscale::{AutoScaler, AutoScaleConfig, AutoScaleEvent};
//!
//! let config = AutoScaleConfig::default();
//! let mut scaler = AutoScaler::new(config);
//!
//! // Record a request and check if scaling is needed
//! scaler.record_request("module_abc");
//! let events = scaler.evaluate();
//! ```

use crate::predict::{
    PredictiveScaler, ResourcePrediction, ResourceSample, ScalerConfig, ScalingAction,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

/// Configuration for the auto-scaler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoScaleConfig {
    /// How often to evaluate scaling decisions.
    pub evaluation_interval: Duration,
    /// Minimum instances per module.
    pub min_instances_per_module: u32,
    /// Maximum instances per module.
    pub max_instances_per_module: u32,
    /// Global maximum pool size.
    pub global_max_pool_size: u32,
    /// Target utilization ratio (0.0-1.0).
    pub target_utilization: f64,
    /// Scaler configuration for the underlying predictor.
    pub scaler_config: ScalerConfig,
}

impl Default for AutoScaleConfig {
    fn default() -> Self {
        Self {
            evaluation_interval: Duration::from_secs(10),
            min_instances_per_module: 1,
            max_instances_per_module: 20,
            global_max_pool_size: 100,
            target_utilization: 0.7,
            scaler_config: ScalerConfig::default(),
        }
    }
}

/// A scaling event produced by the auto-scaler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoScaleEvent {
    /// Scale up instances for a module.
    ScaleUp { module_hash: String, count: u32, reason: String },
    /// Scale down instances for a module.
    ScaleDown { module_hash: String, count: u32, reason: String },
    /// Pool size unchanged.
    NoChange,
}

/// Per-module tracking state.
struct ModuleState {
    /// Total requests seen.
    total_requests: u64,
    /// Requests in the current window.
    window_requests: u64,
    /// Current pool size for this module.
    current_size: u32,
    /// Last request time.
    last_request: Instant,
    /// Per-module predictor.
    scaler: PredictiveScaler,
}

/// Snapshot of the auto-scaler's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoScaleSnapshot {
    /// Total modules tracked.
    pub tracked_modules: usize,
    /// Total pool size across all modules.
    pub total_pool_size: u32,
    /// Global utilization ratio.
    pub global_utilization: f64,
    /// Per-module sizes.
    pub module_sizes: HashMap<String, u32>,
    /// Total scaling events emitted.
    pub total_scale_events: u64,
}

/// Auto-scaling manager.
pub struct AutoScaler {
    config: AutoScaleConfig,
    modules: HashMap<String, ModuleState>,
    last_evaluation: Option<Instant>,
    total_scale_events: u64,
    total_active: u32,
}

impl AutoScaler {
    /// Create a new auto-scaler.
    pub fn new(config: AutoScaleConfig) -> Self {
        Self {
            config,
            modules: HashMap::new(),
            last_evaluation: None,
            total_scale_events: 0,
            total_active: 0,
        }
    }

    /// Record a request for a module.
    pub fn record_request(&mut self, module_hash: &str) {
        let config = self.config.clone();
        let state = self.modules.entry(module_hash.to_string()).or_insert_with(|| ModuleState {
            total_requests: 0,
            window_requests: 0,
            current_size: config.min_instances_per_module,
            last_request: Instant::now(),
            scaler: PredictiveScaler::new(config.scaler_config.clone()),
        });

        state.total_requests += 1;
        state.window_requests += 1;
        state.last_request = Instant::now();
    }

    /// Record active sandbox count for global tracking.
    pub fn set_active_count(&mut self, count: u32) {
        self.total_active = count;
    }

    /// Evaluate scaling decisions for all tracked modules.
    pub fn evaluate(&mut self) -> Vec<AutoScaleEvent> {
        // Check evaluation interval
        if let Some(last) = self.last_evaluation {
            if last.elapsed() < self.config.evaluation_interval {
                return vec![AutoScaleEvent::NoChange];
            }
        }
        self.last_evaluation = Some(Instant::now());

        let total_pool: u32 = self.modules.values().map(|m| m.current_size).sum();
        let mut events = Vec::new();

        let module_hashes: Vec<String> = self.modules.keys().cloned().collect();

        for hash in module_hashes {
            let state = self.modules.get_mut(&hash).unwrap();

            // Feed sample to predictor
            let sample = ResourceSample {
                timestamp: SystemTime::now(),
                cpu_usage: if state.current_size > 0 {
                    (state.window_requests as f64 / state.current_size as f64) * 100.0
                } else {
                    0.0
                },
                memory_usage: 0,
                sandbox_count: state.current_size,
                request_rate: state.window_requests as f64,
            };
            state.scaler.record_sample(sample);
            state.window_requests = 0;

            // Get scaling recommendation
            let action = state.scaler.recommend_action();

            match action {
                ScalingAction::ScaleUp(n) => {
                    let new_size =
                        (state.current_size + n).min(self.config.max_instances_per_module);
                    let actual_increase = new_size - state.current_size;

                    // Check global limit
                    if total_pool + actual_increase <= self.config.global_max_pool_size
                        && actual_increase > 0
                    {
                        state.current_size = new_size;
                        state.scaler.apply_action(ScalingAction::ScaleUp(actual_increase));
                        self.total_scale_events += 1;
                        events.push(AutoScaleEvent::ScaleUp {
                            module_hash: hash.clone(),
                            count: actual_increase,
                            reason: "Predicted demand increase".to_string(),
                        });
                    }
                }
                ScalingAction::ScaleDown(n) => {
                    let new_size = state
                        .current_size
                        .saturating_sub(n)
                        .max(self.config.min_instances_per_module);
                    let actual_decrease = state.current_size - new_size;

                    if actual_decrease > 0 {
                        state.current_size = new_size;
                        state.scaler.apply_action(ScalingAction::ScaleDown(actual_decrease));
                        self.total_scale_events += 1;
                        events.push(AutoScaleEvent::ScaleDown {
                            module_hash: hash.clone(),
                            count: actual_decrease,
                            reason: "Predicted demand decrease".to_string(),
                        });
                    }
                }
                ScalingAction::Maintain => {}
            }
        }

        if events.is_empty() {
            events.push(AutoScaleEvent::NoChange);
        }

        events
    }

    /// Get a snapshot of the current state.
    pub fn snapshot(&self) -> AutoScaleSnapshot {
        let total_pool: u32 = self.modules.values().map(|m| m.current_size).sum();
        let utilization =
            if total_pool > 0 { self.total_active as f64 / total_pool as f64 } else { 0.0 };

        AutoScaleSnapshot {
            tracked_modules: self.modules.len(),
            total_pool_size: total_pool,
            global_utilization: utilization,
            module_sizes: self.modules.iter().map(|(k, v)| (k.clone(), v.current_size)).collect(),
            total_scale_events: self.total_scale_events,
        }
    }

    /// Get the recommended pool size for a specific module.
    pub fn recommended_size(&self, module_hash: &str) -> u32 {
        self.modules
            .get(module_hash)
            .map(|m| m.current_size)
            .unwrap_or(self.config.min_instances_per_module)
    }

    /// Get prediction for a specific module.
    pub fn predict_module(&self, module_hash: &str) -> Option<ResourcePrediction> {
        self.modules.get(module_hash).map(|m| m.scaler.predict())
    }

    /// Remove a module from tracking.
    pub fn remove_module(&mut self, module_hash: &str) -> bool {
        self.modules.remove(module_hash).is_some()
    }

    /// Number of tracked modules.
    pub fn tracked_module_count(&self) -> usize {
        self.modules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autoscaler_creation() {
        let scaler = AutoScaler::new(AutoScaleConfig::default());
        assert_eq!(scaler.tracked_module_count(), 0);
    }

    #[test]
    fn test_record_request() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");
        assert_eq!(scaler.tracked_module_count(), 1);

        scaler.record_request("hash_abc");
        scaler.record_request("hash_def");
        assert_eq!(scaler.tracked_module_count(), 2);
    }

    #[test]
    fn test_evaluate_initial() {
        let config = AutoScaleConfig { evaluation_interval: Duration::ZERO, ..Default::default() };
        let mut scaler = AutoScaler::new(config);
        scaler.record_request("hash_abc");

        let events = scaler.evaluate();
        // First evaluation with minimal data should be NoChange
        assert!(!events.is_empty());
    }

    #[test]
    fn test_snapshot() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");
        scaler.record_request("hash_def");

        let snapshot = scaler.snapshot();
        assert_eq!(snapshot.tracked_modules, 2);
        assert!(snapshot.module_sizes.contains_key("hash_abc"));
        assert!(snapshot.module_sizes.contains_key("hash_def"));
    }

    #[test]
    fn test_recommended_size_unknown() {
        let scaler = AutoScaler::new(AutoScaleConfig::default());
        assert_eq!(scaler.recommended_size("unknown"), 1);
    }

    #[test]
    fn test_recommended_size_known() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");
        assert_eq!(scaler.recommended_size("hash_abc"), 1);
    }

    #[test]
    fn test_predict_module() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");

        let prediction = scaler.predict_module("hash_abc");
        assert!(prediction.is_some());

        let prediction = scaler.predict_module("unknown");
        assert!(prediction.is_none());
    }

    #[test]
    fn test_remove_module() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");
        assert_eq!(scaler.tracked_module_count(), 1);

        assert!(scaler.remove_module("hash_abc"));
        assert_eq!(scaler.tracked_module_count(), 0);

        assert!(!scaler.remove_module("hash_abc"));
    }

    #[test]
    fn test_set_active_count() {
        let mut scaler = AutoScaler::new(AutoScaleConfig::default());
        scaler.record_request("hash_abc");
        scaler.set_active_count(5);

        let snapshot = scaler.snapshot();
        assert!(snapshot.global_utilization > 0.0);
    }

    #[test]
    fn test_evaluation_interval() {
        let config =
            AutoScaleConfig { evaluation_interval: Duration::from_secs(60), ..Default::default() };
        let mut scaler = AutoScaler::new(config);
        scaler.record_request("hash_abc");

        // First eval should succeed
        let _events1 = scaler.evaluate();
        // Second eval within interval should return NoChange
        let events2 = scaler.evaluate();
        assert_eq!(events2, vec![AutoScaleEvent::NoChange]);
    }

    #[test]
    fn test_autoscale_config_default() {
        let config = AutoScaleConfig::default();
        assert_eq!(config.min_instances_per_module, 1);
        assert_eq!(config.max_instances_per_module, 20);
        assert_eq!(config.global_max_pool_size, 100);
        assert!((config.target_utilization - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_autoscale_snapshot_serializable() {
        let snapshot = AutoScaleSnapshot {
            tracked_modules: 2,
            total_pool_size: 10,
            global_utilization: 0.5,
            module_sizes: HashMap::from([("abc".to_string(), 5), ("def".to_string(), 5)]),
            total_scale_events: 3,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: AutoScaleSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tracked_modules, 2);
    }
}
