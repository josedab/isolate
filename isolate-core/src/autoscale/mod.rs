//! # Auto-Scaling Sandbox Pools with Predictive Scaling
//!
//! Intelligent warm pool management using time-series forecasting to predict
//! demand. Supports scale-to-zero, burst capacity, and PID-based control loops.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌──────────────┐
//! │ MetricsRing │────▶│ Forecaster   │────▶│ ScaleDecision│
//! │ (time-series│     │ (EMA/trend)  │     │ (target size)│
//! └─────────────┘     └──────────────┘     └──────────────┘
//!                                                │
//!                                                ▼
//!                                          ┌──────────────┐
//!                                          │ ScaleCtrl    │
//!                                          │ (PID + hyst) │
//!                                          └──────────────┘
//! ```

#![allow(missing_docs)]
mod adaptive;
mod controller;
mod forecast;
mod metrics_ring;

pub use adaptive::{
    AdaptiveConfig, AdaptiveTuner, ExecutionSample, ModuleStats, TunedLimits, TuningChange,
    TuningRecommendation,
};
pub use controller::{ScaleAction, ScaleController, ScaleControllerConfig};
pub use forecast::{DemandForecast, DemandForecaster, ForecastConfig};
pub use metrics_ring::{MetricSample, MetricsRing};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_end_to_end_scaling() {
        let ring = MetricsRing::new(60);
        let forecaster = DemandForecaster::new(ForecastConfig::default());
        let config = ScaleControllerConfig {
            min_pool_size: 1,
            max_pool_size: 100,
            target_utilization: 0.7,
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.3,
            cooldown: Duration::from_secs(0),
        };
        let controller = ScaleController::new(config);

        // Simulate increasing load
        for i in 0..10 {
            ring.record(MetricSample::new(i as f64 * 10.0, i as f64 * 5.0));
        }

        let forecast = forecaster.predict(&ring);
        assert!(forecast.predicted_demand > 0.0);

        let action = controller.decide(5, forecast.predicted_demand as usize, 0.9);
        assert!(matches!(action, ScaleAction::ScaleUp { .. }));
    }
}
