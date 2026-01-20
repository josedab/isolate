use super::metrics_ring::MetricsRing;
use serde::{Deserialize, Serialize};

/// Configuration for the demand forecaster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastConfig {
    /// Smoothing factor for exponential moving average (0.0 - 1.0).
    pub alpha: f64,
    /// Weight for trend component.
    pub trend_weight: f64,
    /// Minimum samples needed before forecasting.
    pub min_samples: usize,
    /// Safety margin multiplier (e.g., 1.2 = 20% headroom).
    pub safety_margin: f64,
}

impl Default for ForecastConfig {
    fn default() -> Self {
        Self {
            alpha: 0.3,
            trend_weight: 0.5,
            min_samples: 5,
            safety_margin: 1.2,
        }
    }
}

/// Result of a demand forecast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandForecast {
    /// Predicted demand (requests per second).
    pub predicted_demand: f64,
    /// Trend direction: positive = increasing, negative = decreasing.
    pub trend: f64,
    /// Confidence level (0.0 - 1.0) based on available data.
    pub confidence: f64,
    /// Recommended pool size based on forecast.
    pub recommended_pool_size: usize,
}

/// Demand forecaster using double exponential smoothing (Holt's method).
///
/// Produces short-horizon demand predictions from time-series metrics,
/// suitable for pool pre-warming decisions.
pub struct DemandForecaster {
    config: ForecastConfig,
}

impl DemandForecaster {
    pub fn new(config: ForecastConfig) -> Self {
        Self { config }
    }

    /// Predict future demand based on current metrics.
    pub fn predict(&self, ring: &MetricsRing) -> DemandForecast {
        let samples = ring.samples();

        if samples.len() < self.config.min_samples {
            return DemandForecast {
                predicted_demand: ring.avg_rps() * self.config.safety_margin,
                trend: 0.0,
                confidence: 0.0,
                recommended_pool_size: 1,
            };
        }

        let rps_values: Vec<f64> = samples.iter().map(|s| s.requests_per_sec).collect();

        // Double exponential smoothing (Holt's linear trend method)
        let (level, trend) = self.holt_smooth(&rps_values);

        // Forecast = level + trend (one step ahead)
        let raw_forecast = level + trend;
        let predicted = (raw_forecast * self.config.safety_margin).max(0.0);

        // Confidence based on data availability and variance
        let confidence = self.compute_confidence(&rps_values);

        // Estimate pool size: assume each sandbox handles ~10 RPS
        let rps_per_sandbox = 10.0;
        let recommended = ((predicted / rps_per_sandbox).ceil() as usize).max(1);

        DemandForecast {
            predicted_demand: predicted,
            trend,
            confidence,
            recommended_pool_size: recommended,
        }
    }

    /// Holt's double exponential smoothing.
    /// Returns (level, trend) for the final time step.
    fn holt_smooth(&self, values: &[f64]) -> (f64, f64) {
        if values.is_empty() {
            return (0.0, 0.0);
        }
        if values.len() == 1 {
            return (values[0], 0.0);
        }

        let alpha = self.config.alpha;
        let beta = self.config.trend_weight;

        let mut level = values[0];
        let mut trend = values[1] - values[0];

        for &val in &values[1..] {
            let prev_level = level;
            level = alpha * val + (1.0 - alpha) * (prev_level + trend);
            trend = beta * (level - prev_level) + (1.0 - beta) * trend;
        }

        (level, trend)
    }

    /// Compute forecast confidence based on data volume and stability.
    fn compute_confidence(&self, values: &[f64]) -> f64 {
        let n = values.len();
        if n == 0 {
            return 0.0;
        }

        // Data volume factor: min_samples = 0.5, 3x min = 1.0
        let volume_factor = ((n as f64) / (self.config.min_samples as f64 * 3.0)).min(1.0);

        // Stability factor: low coefficient of variation = high stability
        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        let cv = if mean > 0.0 {
            variance.sqrt() / mean
        } else {
            1.0
        };
        let stability_factor = (1.0 - cv.min(1.0)).max(0.0);

        (volume_factor * 0.5 + stability_factor * 0.5).clamp(0.0, 1.0)
    }
}

impl Default for DemandForecaster {
    fn default() -> Self {
        Self::new(ForecastConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoscale::metrics_ring::MetricSample;

    fn ring_with_steady_load(rps: f64, count: usize) -> MetricsRing {
        let ring = MetricsRing::new(count + 10);
        for _ in 0..count {
            ring.record(MetricSample::new(rps, 0.5));
        }
        ring
    }

    fn ring_with_increasing_load(count: usize) -> MetricsRing {
        let ring = MetricsRing::new(count + 10);
        for i in 0..count {
            ring.record(MetricSample::new((i + 1) as f64 * 10.0, 0.5));
        }
        ring
    }

    #[test]
    fn test_steady_demand() {
        let ring = ring_with_steady_load(50.0, 20);
        let forecaster = DemandForecaster::default();
        let forecast = forecaster.predict(&ring);

        // Predicted demand should be close to 50 * safety_margin
        assert!(forecast.predicted_demand > 45.0);
        assert!(forecast.predicted_demand < 80.0);
        assert!(forecast.confidence > 0.0);
    }

    #[test]
    fn test_increasing_demand() {
        let ring = ring_with_increasing_load(20);
        let forecaster = DemandForecaster::default();
        let forecast = forecaster.predict(&ring);

        // Trend should be positive
        assert!(forecast.trend > 0.0);
        // Predicted demand should be higher than latest (200)
        assert!(forecast.predicted_demand > 150.0);
    }

    #[test]
    fn test_insufficient_data() {
        let ring = MetricsRing::new(10);
        ring.record(MetricSample::new(10.0, 0.5));
        ring.record(MetricSample::new(20.0, 0.5));

        let forecaster = DemandForecaster::default();
        let forecast = forecaster.predict(&ring);

        assert_eq!(forecast.confidence, 0.0);
        assert!(forecast.predicted_demand > 0.0); // still gives estimate
    }

    #[test]
    fn test_empty_ring() {
        let ring = MetricsRing::new(10);
        let forecaster = DemandForecaster::default();
        let forecast = forecaster.predict(&ring);

        assert_eq!(forecast.predicted_demand, 0.0);
        assert_eq!(forecast.recommended_pool_size, 1);
    }

    #[test]
    fn test_recommended_pool_size() {
        let ring = ring_with_steady_load(100.0, 10);
        let forecaster = DemandForecaster::default();
        let forecast = forecaster.predict(&ring);

        // 100 RPS / 10 RPS per sandbox * 1.2 safety ≈ 12
        assert!(forecast.recommended_pool_size >= 5);
        assert!(forecast.recommended_pool_size <= 30);
    }

    #[test]
    fn test_custom_config() {
        let config = ForecastConfig {
            alpha: 0.5,
            trend_weight: 0.7,
            min_samples: 3,
            safety_margin: 1.5,
        };
        let forecaster = DemandForecaster::new(config);
        let ring = ring_with_steady_load(50.0, 10);
        let forecast = forecaster.predict(&ring);
        // With 1.5x safety margin, demand should be higher
        assert!(forecast.predicted_demand > 50.0);
    }
}
