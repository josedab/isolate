//! Adaptive resource scaling based on execution histograms.
//!
//! Auto-tunes sandbox resource limits (memory, CPU fuel, timeout) based on
//! observed usage patterns. Uses histogram-based analysis with configurable
//! safety guardrails to prevent over- or under-provisioning.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for adaptive resource tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    /// Target percentile for resource sizing (e.g., 0.95 = p95).
    pub target_percentile: f64,
    /// Safety headroom multiplier above the target percentile (e.g., 1.2 = 20% headroom).
    pub headroom_multiplier: f64,
    /// Minimum number of samples before making adjustments.
    pub min_samples: usize,
    /// Maximum allowed reduction per adjustment cycle (0.0-1.0, e.g., 0.5 = max 50% reduction).
    pub max_reduction_pct: f64,
    /// Maximum allowed increase per adjustment cycle (e.g., 2.0 = max 2x increase).
    pub max_increase_multiplier: f64,
    /// Absolute minimum memory limit in bytes.
    pub floor_memory_bytes: u64,
    /// Absolute minimum fuel limit.
    pub floor_fuel: u64,
    /// Absolute minimum timeout in seconds.
    pub floor_timeout_s: u32,
    /// Absolute maximum memory limit in bytes.
    pub ceiling_memory_bytes: u64,
    /// Absolute maximum fuel limit.
    pub ceiling_fuel: u64,
    /// Absolute maximum timeout in seconds.
    pub ceiling_timeout_s: u32,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            target_percentile: 0.95,
            headroom_multiplier: 1.2,
            min_samples: 10,
            max_reduction_pct: 0.5,
            max_increase_multiplier: 2.0,
            floor_memory_bytes: 16 * 1024 * 1024,         // 16MB
            floor_fuel: 100_000,
            floor_timeout_s: 1,
            ceiling_memory_bytes: 4 * 1024 * 1024 * 1024,  // 4GB
            ceiling_fuel: 1_000_000_000,
            ceiling_timeout_s: 3600,
        }
    }
}

/// Observed resource usage from a single execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSample {
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Wall time in seconds.
    pub wall_time_s: f64,
    /// Whether the execution hit a resource limit.
    pub hit_limit: bool,
}

/// Current resource limits being tuned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunedLimits {
    pub memory_bytes: u64,
    pub fuel: u64,
    pub timeout_s: u32,
}

/// A histogram bucket for resource usage analysis.
#[derive(Debug, Clone)]
struct Histogram {
    values: Vec<f64>,
}

impl Histogram {
    fn new() -> Self {
        Self { values: Vec::new() }
    }

    fn add(&mut self, value: f64) {
        self.values.push(value);
    }

    fn len(&self) -> usize {
        self.values.len()
    }

    /// Compute the value at a given percentile (0.0-1.0).
    fn percentile(&self, pct: f64) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let mut sorted = self.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((sorted.len() as f64 - 1.0) * pct).ceil() as usize;
        let idx = idx.min(sorted.len() - 1);
        sorted[idx]
    }

    fn mean(&self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        self.values.iter().sum::<f64>() / self.values.len() as f64
    }

    fn max(&self) -> f64 {
        self.values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Recommendation from the adaptive tuner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningRecommendation {
    /// Recommended new limits.
    pub limits: TunedLimits,
    /// Whether any limit was changed.
    pub changed: bool,
    /// Explanation of each change.
    pub changes: Vec<TuningChange>,
    /// Number of samples analyzed.
    pub samples_analyzed: usize,
}

/// A single limit adjustment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningChange {
    pub resource: String,
    pub old_value: f64,
    pub new_value: f64,
    pub reason: String,
}

/// Adaptive resource tuner for sandbox executions.
///
/// Collects execution samples and uses histogram analysis to recommend
/// optimal resource limits with safety guardrails.
pub struct AdaptiveTuner {
    config: AdaptiveConfig,
    /// Per-module histograms keyed by module identifier.
    modules: parking_lot::Mutex<HashMap<String, ModuleHistograms>>,
}

struct ModuleHistograms {
    memory: Histogram,
    fuel: Histogram,
    wall_time: Histogram,
    limit_hits: u64,
    total_samples: u64,
}

impl ModuleHistograms {
    fn new() -> Self {
        Self {
            memory: Histogram::new(),
            fuel: Histogram::new(),
            wall_time: Histogram::new(),
            limit_hits: 0,
            total_samples: 0,
        }
    }
}

impl AdaptiveTuner {
    /// Create a new adaptive tuner with the given configuration.
    pub fn new(config: AdaptiveConfig) -> Self {
        Self {
            config,
            modules: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Record an execution sample for a module.
    pub fn record_sample(&self, module_id: &str, sample: ExecutionSample) {
        let mut modules = self.modules.lock();
        let hist = modules
            .entry(module_id.to_string())
            .or_insert_with(ModuleHistograms::new);

        hist.memory.add(sample.peak_memory_bytes as f64);
        hist.fuel.add(sample.fuel_consumed as f64);
        hist.wall_time.add(sample.wall_time_s);
        hist.total_samples += 1;
        if sample.hit_limit {
            hist.limit_hits += 1;
        }
    }

    /// Compute tuning recommendations for a module based on collected data.
    pub fn recommend(
        &self,
        module_id: &str,
        current: &TunedLimits,
    ) -> TuningRecommendation {
        let modules = self.modules.lock();
        let hist = match modules.get(module_id) {
            Some(h) => h,
            None => {
                return TuningRecommendation {
                    limits: current.clone(),
                    changed: false,
                    changes: vec![],
                    samples_analyzed: 0,
                };
            }
        };

        if hist.total_samples < self.config.min_samples as u64 {
            return TuningRecommendation {
                limits: current.clone(),
                changed: false,
                changes: vec![],
                samples_analyzed: hist.total_samples as usize,
            };
        }

        let mut changes = Vec::new();
        let mut new_limits = current.clone();

        // Tune memory
        let mem_target = hist.memory.percentile(self.config.target_percentile)
            * self.config.headroom_multiplier;
        let new_mem = self.apply_guardrails_u64(
            current.memory_bytes,
            mem_target as u64,
            self.config.floor_memory_bytes,
            self.config.ceiling_memory_bytes,
        );
        if new_mem != current.memory_bytes {
            changes.push(TuningChange {
                resource: "memory_bytes".into(),
                old_value: current.memory_bytes as f64,
                new_value: new_mem as f64,
                reason: format!(
                    "p{:.0} memory usage: {:.0}MB, recommended: {:.0}MB",
                    self.config.target_percentile * 100.0,
                    hist.memory.percentile(self.config.target_percentile) / (1024.0 * 1024.0),
                    new_mem as f64 / (1024.0 * 1024.0)
                ),
            });
            new_limits.memory_bytes = new_mem;
        }

        // Tune fuel
        let fuel_target = hist.fuel.percentile(self.config.target_percentile)
            * self.config.headroom_multiplier;
        let new_fuel = self.apply_guardrails_u64(
            current.fuel,
            fuel_target as u64,
            self.config.floor_fuel,
            self.config.ceiling_fuel,
        );
        if new_fuel != current.fuel {
            changes.push(TuningChange {
                resource: "fuel".into(),
                old_value: current.fuel as f64,
                new_value: new_fuel as f64,
                reason: format!(
                    "p{:.0} fuel usage: {:.0}, recommended: {}",
                    self.config.target_percentile * 100.0,
                    hist.fuel.percentile(self.config.target_percentile),
                    new_fuel
                ),
            });
            new_limits.fuel = new_fuel;
        }

        // Tune timeout
        let time_target = hist.wall_time.percentile(self.config.target_percentile)
            * self.config.headroom_multiplier;
        let new_timeout = self.apply_guardrails_u64(
            current.timeout_s as u64,
            time_target.ceil() as u64,
            self.config.floor_timeout_s as u64,
            self.config.ceiling_timeout_s as u64,
        ) as u32;
        if new_timeout != current.timeout_s {
            changes.push(TuningChange {
                resource: "timeout_s".into(),
                old_value: current.timeout_s as f64,
                new_value: new_timeout as f64,
                reason: format!(
                    "p{:.0} wall time: {:.1}s, recommended: {}s",
                    self.config.target_percentile * 100.0,
                    hist.wall_time.percentile(self.config.target_percentile),
                    new_timeout
                ),
            });
            new_limits.timeout_s = new_timeout;
        }

        // If executions are hitting limits, ensure we increase
        let hit_rate =
            hist.limit_hits as f64 / hist.total_samples as f64;
        if hit_rate > 0.1 {
            // >10% hitting limits — ensure headroom
            if new_limits.memory_bytes <= current.memory_bytes {
                let bump = (current.memory_bytes as f64 * 1.5) as u64;
                let bumped = bump.min(self.config.ceiling_memory_bytes);
                if bumped > new_limits.memory_bytes {
                    changes.push(TuningChange {
                        resource: "memory_bytes".into(),
                        old_value: new_limits.memory_bytes as f64,
                        new_value: bumped as f64,
                        reason: format!(
                            "{:.0}% of executions hitting limits, increasing memory headroom",
                            hit_rate * 100.0
                        ),
                    });
                    new_limits.memory_bytes = bumped;
                }
            }
        }

        let changed = !changes.is_empty();
        TuningRecommendation {
            limits: new_limits,
            changed,
            changes,
            samples_analyzed: hist.total_samples as usize,
        }
    }

    /// Get summary statistics for a module.
    pub fn module_stats(&self, module_id: &str) -> Option<ModuleStats> {
        let modules = self.modules.lock();
        let hist = modules.get(module_id)?;
        Some(ModuleStats {
            total_samples: hist.total_samples,
            limit_hits: hist.limit_hits,
            memory_mean: hist.memory.mean(),
            memory_p95: hist.memory.percentile(0.95),
            memory_max: hist.memory.max(),
            fuel_mean: hist.fuel.mean(),
            fuel_p95: hist.fuel.percentile(0.95),
            wall_time_mean: hist.wall_time.mean(),
            wall_time_p95: hist.wall_time.percentile(0.95),
        })
    }

    /// Clear collected data for a module.
    pub fn clear(&self, module_id: &str) {
        self.modules.lock().remove(module_id);
    }

    fn apply_guardrails_u64(&self, current: u64, target: u64, floor: u64, ceiling: u64) -> u64 {
        let mut result = target;

        // Apply max reduction
        let min_allowed =
            (current as f64 * (1.0 - self.config.max_reduction_pct)) as u64;
        result = result.max(min_allowed);

        // Apply max increase
        let max_allowed =
            (current as f64 * self.config.max_increase_multiplier) as u64;
        result = result.min(max_allowed);

        // Apply absolute floor and ceiling
        result.clamp(floor, ceiling)
    }
}

/// Summary statistics for a module's execution history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleStats {
    pub total_samples: u64,
    pub limit_hits: u64,
    pub memory_mean: f64,
    pub memory_p95: f64,
    pub memory_max: f64,
    pub fuel_mean: f64,
    pub fuel_p95: f64,
    pub wall_time_mean: f64,
    pub wall_time_p95: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> AdaptiveConfig {
        AdaptiveConfig {
            min_samples: 5,
            ..Default::default()
        }
    }

    fn make_sample(mem_mb: u64, fuel: u64, time_s: f64) -> ExecutionSample {
        ExecutionSample {
            peak_memory_bytes: mem_mb * 1024 * 1024,
            fuel_consumed: fuel,
            wall_time_s: time_s,
            hit_limit: false,
        }
    }

    #[test]
    fn test_basic_recommendation() {
        let tuner = AdaptiveTuner::new(make_config());

        // Record samples with ~50MB memory, ~500K fuel, ~2s
        for _ in 0..10 {
            tuner.record_sample("mod-1", make_sample(50, 500_000, 2.0));
        }

        let current = TunedLimits {
            memory_bytes: 256 * 1024 * 1024, // 256MB
            fuel: 10_000_000,
            timeout_s: 60,
        };

        let rec = tuner.recommend("mod-1", &current);
        assert!(rec.changed);
        // Should recommend reducing from 256MB since p95 is ~50MB
        assert!(rec.limits.memory_bytes < current.memory_bytes);
        // Should recommend reducing fuel from 10M since p95 is ~500K
        assert!(rec.limits.fuel < current.fuel);
    }

    #[test]
    fn test_not_enough_samples() {
        let tuner = AdaptiveTuner::new(make_config());

        // Only 3 samples, min_samples is 5
        for _ in 0..3 {
            tuner.record_sample("mod-1", make_sample(50, 500_000, 2.0));
        }

        let current = TunedLimits {
            memory_bytes: 256 * 1024 * 1024,
            fuel: 10_000_000,
            timeout_s: 60,
        };

        let rec = tuner.recommend("mod-1", &current);
        assert!(!rec.changed);
        assert_eq!(rec.samples_analyzed, 3);
    }

    #[test]
    fn test_max_reduction_guardrail() {
        let mut config = make_config();
        config.max_reduction_pct = 0.5; // Max 50% reduction
        let tuner = AdaptiveTuner::new(config);

        // Very low usage
        for _ in 0..10 {
            tuner.record_sample("mod-1", make_sample(1, 100, 0.1));
        }

        let current = TunedLimits {
            memory_bytes: 1024 * 1024 * 1024, // 1GB
            fuel: 100_000_000,
            timeout_s: 300,
        };

        let rec = tuner.recommend("mod-1", &current);
        // Should not reduce by more than 50%
        assert!(rec.limits.memory_bytes >= current.memory_bytes / 2);
    }

    #[test]
    fn test_floor_enforced() {
        let mut config = make_config();
        config.floor_memory_bytes = 32 * 1024 * 1024; // 32MB floor
        config.max_reduction_pct = 1.0; // Allow full reduction to test floor
        let tuner = AdaptiveTuner::new(config);

        // Tiny usage
        for _ in 0..10 {
            tuner.record_sample("mod-1", make_sample(1, 100, 0.01));
        }

        let current = TunedLimits {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 1_000_000,
            timeout_s: 30,
        };

        let rec = tuner.recommend("mod-1", &current);
        assert!(rec.limits.memory_bytes >= 32 * 1024 * 1024);
    }

    #[test]
    fn test_ceiling_enforced() {
        let mut config = make_config();
        config.ceiling_memory_bytes = 512 * 1024 * 1024; // 512MB ceiling
        let tuner = AdaptiveTuner::new(config);

        // High usage
        for _ in 0..10 {
            tuner.record_sample("mod-1", make_sample(400, 50_000_000, 100.0));
        }

        let current = TunedLimits {
            memory_bytes: 256 * 1024 * 1024,
            fuel: 10_000_000,
            timeout_s: 60,
        };

        let rec = tuner.recommend("mod-1", &current);
        assert!(rec.limits.memory_bytes <= 512 * 1024 * 1024);
    }

    #[test]
    fn test_limit_hit_bump() {
        let tuner = AdaptiveTuner::new(make_config());

        // Most executions hitting limits
        for _ in 0..10 {
            tuner.record_sample("mod-1", ExecutionSample {
                peak_memory_bytes: 100 * 1024 * 1024,
                fuel_consumed: 500_000,
                wall_time_s: 2.0,
                hit_limit: true,
            });
        }

        let current = TunedLimits {
            memory_bytes: 100 * 1024 * 1024,
            fuel: 500_000,
            timeout_s: 2,
        };

        let rec = tuner.recommend("mod-1", &current);
        assert!(rec.changed);
        // Should increase memory due to high limit-hit rate
        assert!(rec.limits.memory_bytes > current.memory_bytes);
    }

    #[test]
    fn test_module_stats() {
        let tuner = AdaptiveTuner::new(make_config());

        tuner.record_sample("mod-1", make_sample(50, 500_000, 2.0));
        tuner.record_sample("mod-1", make_sample(100, 1_000_000, 5.0));

        let stats = tuner.module_stats("mod-1").unwrap();
        assert_eq!(stats.total_samples, 2);
        assert_eq!(stats.limit_hits, 0);
        assert!(stats.memory_mean > 0.0);
        assert!(stats.fuel_mean > 0.0);
    }

    #[test]
    fn test_unknown_module() {
        let tuner = AdaptiveTuner::new(make_config());
        assert!(tuner.module_stats("unknown").is_none());

        let current = TunedLimits {
            memory_bytes: 128 * 1024 * 1024,
            fuel: 1_000_000,
            timeout_s: 30,
        };
        let rec = tuner.recommend("unknown", &current);
        assert!(!rec.changed);
    }

    #[test]
    fn test_clear_module_data() {
        let tuner = AdaptiveTuner::new(make_config());
        tuner.record_sample("mod-1", make_sample(50, 500_000, 2.0));
        assert!(tuner.module_stats("mod-1").is_some());

        tuner.clear("mod-1");
        assert!(tuner.module_stats("mod-1").is_none());
    }

    #[test]
    fn test_histogram_percentile() {
        let mut h = Histogram::new();
        for i in 1..=100 {
            h.add(i as f64);
        }
        assert_eq!(h.len(), 100);
        assert!((h.percentile(0.5) - 50.0).abs() < 2.0);
        assert!((h.percentile(0.95) - 95.0).abs() < 2.0);
        assert_eq!(h.percentile(1.0), 100.0);
    }

    #[test]
    fn test_stable_recommendations() {
        let tuner = AdaptiveTuner::new(make_config());

        // Consistent usage
        for _ in 0..20 {
            tuner.record_sample("mod-1", make_sample(100, 1_000_000, 5.0));
        }

        let current = TunedLimits {
            memory_bytes: 120 * 1024 * 1024, // Close to actual usage * headroom
            fuel: 1_200_000,                  // Close to actual * headroom
            timeout_s: 7,                      // Close to actual * headroom
        };

        let rec = tuner.recommend("mod-1", &current);
        // Changes should be minimal since current is already well-tuned
        for change in &rec.changes {
            let pct_change = ((change.new_value - change.old_value) / change.old_value).abs();
            assert!(pct_change < 0.5, "change too large for {}: {:.0}%", change.resource, pct_change * 100.0);
        }
    }
}
