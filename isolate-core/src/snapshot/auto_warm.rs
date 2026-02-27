//! Auto-warming heuristics for snapshot-based fast restore.
//!
//! Tracks module usage patterns and automatically creates/refreshes snapshots
//! for frequently-used modules to ensure sub-millisecond warm starts.

use crate::config::ModuleHash;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Configuration for auto-warming behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoWarmConfig {
    /// Minimum executions before a module is considered "hot".
    pub hot_threshold: u64,
    /// Time window for measuring execution frequency.
    pub window_duration: Duration,
    /// Maximum number of modules to keep warm simultaneously.
    pub max_warm_modules: usize,
    /// Interval between auto-warming sweeps.
    pub sweep_interval: Duration,
}

impl Default for AutoWarmConfig {
    fn default() -> Self {
        Self {
            hot_threshold: 10,
            window_duration: Duration::from_secs(300), // 5 minutes
            max_warm_modules: 50,
            sweep_interval: Duration::from_secs(60),
        }
    }
}

/// Tracks module access patterns for auto-warming decisions.
pub struct AccessTracker {
    config: AutoWarmConfig,
    modules: RwLock<HashMap<ModuleHash, ModuleAccessStats>>,
    total_accesses: AtomicU64,
}

/// Per-module access statistics.
#[derive(Debug, Clone)]
struct ModuleAccessStats {
    access_count: u64,
    window_start: Instant,
    last_access: Instant,
    avg_cold_start_ms: f64,
}

/// Result of an auto-warming analysis sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmingRecommendation {
    /// Modules that should be pre-warmed (hot modules without snapshots).
    pub modules_to_warm: Vec<ModuleHash>,
    /// Modules that can be evicted (cold modules with stale snapshots).
    pub modules_to_evict: Vec<ModuleHash>,
    /// Total number of tracked modules.
    pub total_tracked: usize,
    /// Number of modules classified as "hot".
    pub hot_count: usize,
}

impl AccessTracker {
    /// Create a new access tracker.
    pub fn new(config: AutoWarmConfig) -> Self {
        Self { config, modules: RwLock::new(HashMap::new()), total_accesses: AtomicU64::new(0) }
    }

    /// Record an access to a module.
    pub fn record_access(&self, module_hash: &ModuleHash, cold_start_ms: f64) {
        let mut modules = self.modules.write();
        let now = Instant::now();

        let stats = modules.entry(module_hash.clone()).or_insert_with(|| ModuleAccessStats {
            access_count: 0,
            window_start: now,
            last_access: now,
            avg_cold_start_ms: 0.0,
        });

        // Reset window if expired
        if now.duration_since(stats.window_start) > self.config.window_duration {
            stats.access_count = 0;
            stats.window_start = now;
        }

        stats.access_count += 1;
        stats.last_access = now;
        // Exponential moving average of cold start times
        stats.avg_cold_start_ms = stats.avg_cold_start_ms * 0.8 + cold_start_ms * 0.2;

        self.total_accesses.fetch_add(1, Ordering::Relaxed);
    }

    /// Analyze access patterns and produce warming recommendations.
    pub fn analyze(&self) -> WarmingRecommendation {
        let modules = self.modules.read();
        let now = Instant::now();

        let mut hot_modules: Vec<(ModuleHash, &ModuleAccessStats)> = modules
            .iter()
            .filter(|(_, stats)| {
                stats.access_count >= self.config.hot_threshold
                    && now.duration_since(stats.window_start) <= self.config.window_duration
            })
            .map(|(hash, stats)| (hash.clone(), stats))
            .collect();

        // Sort by access count (most accessed first)
        hot_modules.sort_by(|a, b| b.1.access_count.cmp(&a.1.access_count));

        let modules_to_warm: Vec<ModuleHash> = hot_modules
            .iter()
            .take(self.config.max_warm_modules)
            .map(|(hash, _)| hash.clone())
            .collect();

        let hot_set: std::collections::HashSet<&ModuleHash> =
            hot_modules.iter().map(|(h, _)| h).collect();

        let modules_to_evict: Vec<ModuleHash> = modules
            .iter()
            .filter(|(hash, stats)| {
                !hot_set.contains(hash)
                    && now.duration_since(stats.last_access) > self.config.window_duration * 2
            })
            .map(|(hash, _)| hash.clone())
            .collect();

        WarmingRecommendation {
            hot_count: hot_modules.len(),
            modules_to_warm,
            modules_to_evict,
            total_tracked: modules.len(),
        }
    }

    /// Check if a specific module is considered hot.
    pub fn is_hot(&self, module_hash: &ModuleHash) -> bool {
        let modules = self.modules.read();
        modules.get(module_hash).map_or(false, |stats| {
            stats.access_count >= self.config.hot_threshold
                && stats.window_start.elapsed() <= self.config.window_duration
        })
    }

    /// Get total tracked accesses.
    pub fn total_accesses(&self) -> u64 {
        self.total_accesses.load(Ordering::Relaxed)
    }

    /// Clear all tracking data.
    pub fn clear(&self) {
        self.modules.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_warm_config_defaults() {
        let config = AutoWarmConfig::default();
        assert_eq!(config.hot_threshold, 10);
        assert_eq!(config.max_warm_modules, 50);
    }

    #[test]
    fn test_access_tracker_basic() {
        let tracker = AccessTracker::new(AutoWarmConfig { hot_threshold: 3, ..Default::default() });
        let hash = ModuleHash("test_module".to_string());

        // Not hot yet
        assert!(!tracker.is_hot(&hash));

        // Record accesses
        for _ in 0..3 {
            tracker.record_access(&hash, 2.5);
        }

        // Should be hot now
        assert!(tracker.is_hot(&hash));
        assert_eq!(tracker.total_accesses(), 3);
    }

    #[test]
    fn test_warming_recommendation() {
        let tracker = AccessTracker::new(AutoWarmConfig {
            hot_threshold: 2,
            max_warm_modules: 5,
            ..Default::default()
        });

        let hot = ModuleHash("hot_module".to_string());
        let cold = ModuleHash("cold_module".to_string());

        // Make one hot
        for _ in 0..5 {
            tracker.record_access(&hot, 1.0);
        }
        // One cold access
        tracker.record_access(&cold, 3.0);

        let rec = tracker.analyze();
        assert_eq!(rec.hot_count, 1);
        assert!(rec.modules_to_warm.contains(&hot));
        assert!(!rec.modules_to_warm.contains(&cold));
    }

    #[test]
    fn test_tracker_clear() {
        let tracker = AccessTracker::new(AutoWarmConfig::default());
        let hash = ModuleHash("module".to_string());
        tracker.record_access(&hash, 1.0);
        assert_eq!(tracker.total_accesses(), 1);

        tracker.clear();
        assert!(!tracker.is_hot(&hash));
    }
}
