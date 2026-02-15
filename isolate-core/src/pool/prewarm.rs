//! Pre-warming strategy engine for the warm pool.
//!
//! Decides when and how many sandbox instances to pre-warm based on
//! configurable strategies and usage-pattern analysis.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Strategy for deciding how many instances to pre-warm.
#[derive(Debug, Clone)]
pub enum PreWarmStrategy {
    /// Maintain a fixed number of warm instances per module.
    Fixed { target_count: usize },
    /// Scale based on recent request rate (instances = rate × lead_time).
    RateBased { window: Duration, lead_time: Duration, min_instances: usize, max_instances: usize },
    /// Scale based on time-of-day patterns learned from history.
    Predictive { history_window: Duration, lookahead: Duration, min_instances: usize },
}

impl Default for PreWarmStrategy {
    fn default() -> Self {
        Self::Fixed { target_count: 2 }
    }
}

/// Configuration for the pre-warming engine.
#[derive(Debug, Clone)]
pub struct PreWarmConfig {
    /// Default strategy for all modules.
    pub default_strategy: PreWarmStrategy,
    /// Per-module strategy overrides.
    pub module_strategies: HashMap<String, PreWarmStrategy>,
    /// How often to re-evaluate pre-warming decisions.
    pub evaluation_interval: Duration,
    /// Cooldown period after a scale-down before scaling down again.
    pub scale_down_cooldown: Duration,
}

impl Default for PreWarmConfig {
    fn default() -> Self {
        Self {
            default_strategy: PreWarmStrategy::default(),
            module_strategies: HashMap::new(),
            evaluation_interval: Duration::from_secs(10),
            scale_down_cooldown: Duration::from_secs(60),
        }
    }
}

/// A decision about how many instances to target for a module.
#[derive(Debug, Clone)]
pub struct PreWarmDecision {
    /// Module name.
    pub module_name: String,
    /// Target number of warm instances.
    pub target_instances: usize,
    /// Current warm instances.
    pub current_instances: usize,
    /// Action to take.
    pub action: PreWarmAction,
    /// Reason for the decision.
    pub reason: String,
}

/// Action resulting from a pre-warming evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreWarmAction {
    /// Create additional warm instances.
    ScaleUp(usize),
    /// No change needed.
    NoChange,
    /// Allow excess instances to be evicted (don't replenish).
    ScaleDown(usize),
}

/// Tracks request rates for rate-based pre-warming.
struct RequestTracker {
    /// Timestamps of recent requests (ring buffer style).
    timestamps: Vec<Instant>,
    /// Head pointer for the ring buffer.
    head: usize,
    /// Number of valid entries.
    count: usize,
}

impl RequestTracker {
    fn new(capacity: usize) -> Self {
        Self { timestamps: vec![Instant::now(); capacity], head: 0, count: 0 }
    }

    fn record_request(&mut self) {
        self.timestamps[self.head] = Instant::now();
        self.head = (self.head + 1) % self.timestamps.len();
        if self.count < self.timestamps.len() {
            self.count += 1;
        }
    }

    fn request_rate(&self, window: Duration) -> f64 {
        let now = Instant::now();
        let cutoff = now - window;
        let recent = self.timestamps.iter().filter(|&&t| t > cutoff).count();
        if window.as_secs_f64() > 0.0 {
            recent as f64 / window.as_secs_f64()
        } else {
            0.0
        }
    }
}

/// Pre-warming engine that evaluates strategies and produces decisions.
pub struct PreWarmEngine {
    config: PreWarmConfig,
    trackers: HashMap<String, RequestTracker>,
    last_scale_down: HashMap<String, Instant>,
    last_evaluation: Option<Instant>,
}

impl PreWarmEngine {
    /// Create a new pre-warming engine.
    pub fn new(config: PreWarmConfig) -> Self {
        Self {
            config,
            trackers: HashMap::new(),
            last_scale_down: HashMap::new(),
            last_evaluation: None,
        }
    }

    /// Record a request for a module (call this on every sandbox acquisition).
    pub fn record_request(&mut self, module_name: &str) {
        self.trackers
            .entry(module_name.to_string())
            .or_insert_with(|| RequestTracker::new(1000))
            .record_request();
    }

    /// Check if it's time to re-evaluate pre-warming.
    pub fn should_evaluate(&self) -> bool {
        match self.last_evaluation {
            Some(last) => last.elapsed() >= self.config.evaluation_interval,
            None => true,
        }
    }

    /// Evaluate all modules and produce pre-warming decisions.
    pub fn evaluate(
        &mut self,
        current_counts: &HashMap<String, usize>,
    ) -> Vec<PreWarmDecision> {
        self.last_evaluation = Some(Instant::now());
        let mut decisions = Vec::new();

        for (module_name, &current) in current_counts {
            let strategy = self
                .config
                .module_strategies
                .get(module_name)
                .unwrap_or(&self.config.default_strategy);

            let target = self.compute_target(module_name, strategy);
            let action = self.compute_action(module_name, current, target);

            decisions.push(PreWarmDecision {
                module_name: module_name.clone(),
                target_instances: target,
                current_instances: current,
                action: action.clone(),
                reason: match &action {
                    PreWarmAction::ScaleUp(n) => {
                        format!("need {} more instances (target={}, current={})", n, target, current)
                    }
                    PreWarmAction::NoChange => "at target level".to_string(),
                    PreWarmAction::ScaleDown(n) => {
                        format!(
                            "{} excess instances (target={}, current={})",
                            n, target, current
                        )
                    }
                },
            });
        }

        decisions
    }

    /// Evaluate a single module.
    pub fn evaluate_module(
        &mut self,
        module_name: &str,
        current_count: usize,
    ) -> PreWarmDecision {
        let strategy = self
            .config
            .module_strategies
            .get(module_name)
            .unwrap_or(&self.config.default_strategy);

        let target = self.compute_target(module_name, strategy);
        let action = self.compute_action(module_name, current_count, target);

        PreWarmDecision {
            module_name: module_name.to_string(),
            target_instances: target,
            current_instances: current_count,
            action: action.clone(),
            reason: match &action {
                PreWarmAction::ScaleUp(n) => format!("need {} more instances", n),
                PreWarmAction::NoChange => "at target level".to_string(),
                PreWarmAction::ScaleDown(n) => format!("{} excess instances", n),
            },
        }
    }

    /// Update the strategy for a module at runtime.
    pub fn set_module_strategy(&mut self, module_name: impl Into<String>, strategy: PreWarmStrategy) {
        self.config.module_strategies.insert(module_name.into(), strategy);
    }

    fn compute_target(&self, module_name: &str, strategy: &PreWarmStrategy) -> usize {
        match strategy {
            PreWarmStrategy::Fixed { target_count } => *target_count,
            PreWarmStrategy::RateBased { window, lead_time, min_instances, max_instances } => {
                let rate = self
                    .trackers
                    .get(module_name)
                    .map(|t| t.request_rate(*window))
                    .unwrap_or(0.0);
                let needed = (rate * lead_time.as_secs_f64()).ceil() as usize;
                needed.clamp(*min_instances, *max_instances)
            }
            PreWarmStrategy::Predictive { min_instances, .. } => {
                // Simplified: use recent rate as predictor, fall back to min
                let rate = self
                    .trackers
                    .get(module_name)
                    .map(|t| t.request_rate(Duration::from_secs(60)))
                    .unwrap_or(0.0);
                let needed = (rate * 2.0).ceil() as usize;
                needed.max(*min_instances)
            }
        }
    }

    fn compute_action(
        &mut self,
        module_name: &str,
        current: usize,
        target: usize,
    ) -> PreWarmAction {
        if current < target {
            PreWarmAction::ScaleUp(target - current)
        } else if current > target {
            // Respect scale-down cooldown
            let can_scale_down = self
                .last_scale_down
                .get(module_name)
                .map(|t| t.elapsed() >= self.config.scale_down_cooldown)
                .unwrap_or(true);
            if can_scale_down {
                self.last_scale_down.insert(module_name.to_string(), Instant::now());
                PreWarmAction::ScaleDown(current - target)
            } else {
                PreWarmAction::NoChange
            }
        } else {
            PreWarmAction::NoChange
        }
    }
}

impl Default for PreWarmEngine {
    fn default() -> Self {
        Self::new(PreWarmConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_strategy() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 5 },
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        let decision = engine.evaluate_module("test", 2);
        assert_eq!(decision.target_instances, 5);
        assert_eq!(decision.action, PreWarmAction::ScaleUp(3));
    }

    #[test]
    fn test_no_change_at_target() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 3 },
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        let decision = engine.evaluate_module("test", 3);
        assert_eq!(decision.action, PreWarmAction::NoChange);
    }

    #[test]
    fn test_scale_down() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 2 },
            scale_down_cooldown: Duration::from_millis(0),
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        let decision = engine.evaluate_module("test", 5);
        assert_eq!(decision.action, PreWarmAction::ScaleDown(3));
    }

    #[test]
    fn test_scale_down_cooldown() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 1 },
            scale_down_cooldown: Duration::from_secs(60),
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        // First scale-down should work
        let d1 = engine.evaluate_module("test", 5);
        assert_eq!(d1.action, PreWarmAction::ScaleDown(4));

        // Second should be blocked by cooldown
        let d2 = engine.evaluate_module("test", 5);
        assert_eq!(d2.action, PreWarmAction::NoChange);
    }

    #[test]
    fn test_rate_based_strategy() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::RateBased {
                window: Duration::from_secs(60),
                lead_time: Duration::from_secs(5),
                min_instances: 1,
                max_instances: 20,
            },
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        // Record some requests
        for _ in 0..10 {
            engine.record_request("test");
        }

        let decision = engine.evaluate_module("test", 0);
        assert!(decision.target_instances >= 1);
        assert!(matches!(decision.action, PreWarmAction::ScaleUp(_)));
    }

    #[test]
    fn test_per_module_strategy() {
        let mut config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 2 },
            ..Default::default()
        };
        config
            .module_strategies
            .insert("special".to_string(), PreWarmStrategy::Fixed { target_count: 10 });
        let mut engine = PreWarmEngine::new(config);

        let normal = engine.evaluate_module("normal", 0);
        assert_eq!(normal.target_instances, 2);

        let special = engine.evaluate_module("special", 0);
        assert_eq!(special.target_instances, 10);
    }

    #[test]
    fn test_batch_evaluation() {
        let config = PreWarmConfig {
            default_strategy: PreWarmStrategy::Fixed { target_count: 3 },
            scale_down_cooldown: Duration::from_millis(0),
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        let mut counts = HashMap::new();
        counts.insert("mod-a".to_string(), 1);
        counts.insert("mod-b".to_string(), 5);

        let decisions = engine.evaluate(&counts);
        assert_eq!(decisions.len(), 2);

        let a = decisions.iter().find(|d| d.module_name == "mod-a").unwrap();
        assert_eq!(a.action, PreWarmAction::ScaleUp(2));

        let b = decisions.iter().find(|d| d.module_name == "mod-b").unwrap();
        assert_eq!(b.action, PreWarmAction::ScaleDown(2));
    }

    #[test]
    fn test_should_evaluate_timing() {
        let config = PreWarmConfig {
            evaluation_interval: Duration::from_millis(10),
            ..Default::default()
        };
        let mut engine = PreWarmEngine::new(config);

        assert!(engine.should_evaluate()); // never evaluated
        engine.evaluate(&HashMap::new());
        assert!(!engine.should_evaluate()); // just evaluated

        std::thread::sleep(Duration::from_millis(15));
        assert!(engine.should_evaluate()); // interval passed
    }

    #[test]
    fn test_set_module_strategy_runtime() {
        let mut engine = PreWarmEngine::default();
        engine.set_module_strategy("dynamic", PreWarmStrategy::Fixed { target_count: 7 });

        let decision = engine.evaluate_module("dynamic", 0);
        assert_eq!(decision.target_instances, 7);
    }
}
