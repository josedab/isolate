use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Configuration for the scale controller.
#[derive(Debug, Clone)]
pub struct ScaleControllerConfig {
    pub min_pool_size: usize,
    pub max_pool_size: usize,
    pub target_utilization: f64,
    pub scale_up_threshold: f64,
    pub scale_down_threshold: f64,
    pub cooldown: Duration,
}

impl Default for ScaleControllerConfig {
    fn default() -> Self {
        Self {
            min_pool_size: 1,
            max_pool_size: 100,
            target_utilization: 0.7,
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.3,
            cooldown: Duration::from_secs(30),
        }
    }
}

/// Scaling action decided by the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleAction {
    /// No action needed, pool is right-sized.
    NoOp,
    /// Scale up to the target size.
    ScaleUp { current: usize, target: usize },
    /// Scale down to the target size.
    ScaleDown { current: usize, target: usize },
    /// Scale to zero (idle timeout).
    ScaleToZero,
}

/// PID-inspired controller with hysteresis for pool scaling decisions.
///
/// Prevents thrashing by requiring sustained demand changes before scaling
/// and enforcing cooldown periods between actions.
pub struct ScaleController {
    config: ScaleControllerConfig,
    last_action: parking_lot::Mutex<Option<Instant>>,
    consecutive_up: parking_lot::Mutex<u32>,
    consecutive_down: parking_lot::Mutex<u32>,
}

impl ScaleController {
    pub fn new(config: ScaleControllerConfig) -> Self {
        Self {
            config,
            last_action: parking_lot::Mutex::new(None),
            consecutive_up: parking_lot::Mutex::new(0),
            consecutive_down: parking_lot::Mutex::new(0),
        }
    }

    /// Decide what scaling action to take based on current state.
    pub fn decide(
        &self,
        current_size: usize,
        predicted_demand: usize,
        current_utilization: f64,
    ) -> ScaleAction {
        // Check cooldown
        if let Some(last) = *self.last_action.lock() {
            if last.elapsed() < self.config.cooldown {
                return ScaleAction::NoOp;
            }
        }

        // Scale to zero check: no demand and low utilization
        if predicted_demand == 0 && current_utilization < 0.01 && current_size > 0 {
            *self.consecutive_down.lock() += 1;
            *self.consecutive_up.lock() = 0;
            if *self.consecutive_down.lock() >= 3 {
                self.mark_action();
                return ScaleAction::ScaleToZero;
            }
            return ScaleAction::NoOp;
        }

        // Target size based on predicted demand
        let target_by_demand = predicted_demand.max(self.config.min_pool_size);

        // Target size based on utilization
        let target_by_util = if current_utilization > self.config.scale_up_threshold {
            ((current_size as f64 * current_utilization / self.config.target_utilization).ceil()
                as usize)
                .max(current_size + 1)
        } else if current_utilization < self.config.scale_down_threshold && current_size > 1 {
            ((current_size as f64 * current_utilization / self.config.target_utilization).ceil()
                as usize)
                .max(1)
        } else {
            current_size
        };

        // Take the larger of demand-based and utilization-based targets
        let target = target_by_demand
            .max(target_by_util)
            .clamp(self.config.min_pool_size, self.config.max_pool_size);

        if target > current_size {
            *self.consecutive_up.lock() += 1;
            *self.consecutive_down.lock() = 0;
            self.mark_action();
            ScaleAction::ScaleUp { current: current_size, target }
        } else if target < current_size && current_utilization < self.config.scale_down_threshold {
            *self.consecutive_down.lock() += 1;
            *self.consecutive_up.lock() = 0;
            // Require 2 consecutive scale-down signals before acting (hysteresis)
            if *self.consecutive_down.lock() >= 2 {
                self.mark_action();
                ScaleAction::ScaleDown { current: current_size, target }
            } else {
                ScaleAction::NoOp
            }
        } else {
            *self.consecutive_up.lock() = 0;
            *self.consecutive_down.lock() = 0;
            ScaleAction::NoOp
        }
    }

    fn mark_action(&self) {
        *self.last_action.lock() = Some(Instant::now());
    }

    /// Get current configuration.
    pub fn config(&self) -> &ScaleControllerConfig {
        &self.config
    }
}

impl Default for ScaleController {
    fn default() -> Self {
        Self::new(ScaleControllerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ScaleControllerConfig {
        ScaleControllerConfig {
            min_pool_size: 1,
            max_pool_size: 50,
            target_utilization: 0.7,
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.3,
            cooldown: Duration::from_secs(0), // no cooldown for tests
        }
    }

    #[test]
    fn test_scale_up_high_utilization() {
        let ctrl = ScaleController::new(test_config());
        let action = ctrl.decide(5, 10, 0.9);
        assert!(matches!(action, ScaleAction::ScaleUp { .. }));
    }

    #[test]
    fn test_scale_up_high_demand() {
        let ctrl = ScaleController::new(test_config());
        let action = ctrl.decide(5, 20, 0.5);
        assert!(matches!(action, ScaleAction::ScaleUp { .. }));
    }

    #[test]
    fn test_no_op_normal_load() {
        let ctrl = ScaleController::new(test_config());
        let action = ctrl.decide(10, 10, 0.6);
        assert_eq!(action, ScaleAction::NoOp);
    }

    #[test]
    fn test_scale_down_low_utilization() {
        let ctrl = ScaleController::new(test_config());
        // Need 2 consecutive scale-down signals
        ctrl.decide(10, 1, 0.1);
        let action = ctrl.decide(10, 1, 0.1);
        assert!(matches!(action, ScaleAction::ScaleDown { .. }));
    }

    #[test]
    fn test_scale_to_zero() {
        let ctrl = ScaleController::new(test_config());
        // Need 3 consecutive zero-demand signals
        ctrl.decide(1, 0, 0.0);
        ctrl.decide(1, 0, 0.0);
        let action = ctrl.decide(1, 0, 0.0);
        assert_eq!(action, ScaleAction::ScaleToZero);
    }

    #[test]
    fn test_respects_min_pool_size() {
        let mut cfg = test_config();
        cfg.min_pool_size = 3;
        let ctrl = ScaleController::new(cfg);
        let action = ctrl.decide(1, 1, 0.1);
        assert!(matches!(action, ScaleAction::ScaleUp { target: 3, .. }));
    }

    #[test]
    fn test_respects_max_pool_size() {
        let mut cfg = test_config();
        cfg.max_pool_size = 10;
        let ctrl = ScaleController::new(cfg);
        let action = ctrl.decide(8, 100, 0.95);
        if let ScaleAction::ScaleUp { target, .. } = action {
            assert!(target <= 10);
        }
    }

    #[test]
    fn test_cooldown() {
        let mut cfg = test_config();
        cfg.cooldown = Duration::from_secs(60);
        let ctrl = ScaleController::new(cfg);

        let action1 = ctrl.decide(5, 20, 0.9);
        assert!(matches!(action1, ScaleAction::ScaleUp { .. }));

        // Second call during cooldown should be NoOp
        let action2 = ctrl.decide(5, 20, 0.9);
        assert_eq!(action2, ScaleAction::NoOp);
    }
}
