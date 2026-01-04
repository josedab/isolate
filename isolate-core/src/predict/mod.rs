//! Predictive Resource Scaling
//!
//! Uses ML to predict resource needs and pre-allocate:
//! - Time series forecasting for workload prediction
//! - Automatic sandbox pool sizing
//! - Proactive resource allocation

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Resource metric sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSample {
    /// Timestamp.
    pub timestamp: std::time::SystemTime,
    /// CPU usage percentage.
    pub cpu_usage: f64,
    /// Memory usage bytes.
    pub memory_usage: u64,
    /// Active sandbox count.
    pub sandbox_count: u32,
    /// Request rate per second.
    pub request_rate: f64,
}

/// Prediction result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePrediction {
    /// Predicted CPU usage.
    pub cpu_usage: f64,
    /// Predicted memory usage.
    pub memory_usage: u64,
    /// Predicted sandbox count.
    pub sandbox_count: u32,
    /// Confidence (0.0-1.0).
    pub confidence: f64,
    /// Prediction horizon.
    pub horizon: Duration,
    /// Recommended pool size.
    pub recommended_pool_size: u32,
}

/// Scaling action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalingAction {
    /// Scale up by amount.
    ScaleUp(u32),
    /// Scale down by amount.
    ScaleDown(u32),
    /// Maintain current size.
    Maintain,
}

/// Predictive scaler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalerConfig {
    /// Sample window size.
    pub sample_window: usize,
    /// Prediction horizon.
    pub prediction_horizon: Duration,
    /// Scale up threshold.
    pub scale_up_threshold: f64,
    /// Scale down threshold.
    pub scale_down_threshold: f64,
    /// Minimum pool size.
    pub min_pool_size: u32,
    /// Maximum pool size.
    pub max_pool_size: u32,
    /// Cooldown between scaling actions.
    pub cooldown: Duration,
}

impl Default for ScalerConfig {
    fn default() -> Self {
        Self {
            sample_window: 60,
            prediction_horizon: Duration::from_secs(300),
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.3,
            min_pool_size: 5,
            max_pool_size: 100,
            cooldown: Duration::from_secs(60),
        }
    }
}

/// Predictive resource scaler.
pub struct PredictiveScaler {
    config: ScalerConfig,
    samples: VecDeque<ResourceSample>,
    current_pool_size: u32,
    last_scaling_action: Option<Instant>,
}

impl PredictiveScaler {
    /// Create a new predictive scaler.
    pub fn new(config: ScalerConfig) -> Self {
        let sample_window = config.sample_window;
        let min_pool_size = config.min_pool_size;
        Self {
            config,
            samples: VecDeque::with_capacity(sample_window),
            current_pool_size: min_pool_size,
            last_scaling_action: None,
        }
    }

    /// Record a resource sample.
    pub fn record_sample(&mut self, sample: ResourceSample) {
        if self.samples.len() >= self.config.sample_window {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Get prediction based on current data.
    pub fn predict(&self) -> ResourcePrediction {
        if self.samples.is_empty() {
            return ResourcePrediction {
                cpu_usage: 0.0,
                memory_usage: 0,
                sandbox_count: self.config.min_pool_size,
                confidence: 0.0,
                horizon: self.config.prediction_horizon,
                recommended_pool_size: self.config.min_pool_size,
            };
        }

        // Simple moving average prediction
        let avg_cpu =
            self.samples.iter().map(|s| s.cpu_usage).sum::<f64>() / self.samples.len() as f64;
        let avg_mem =
            self.samples.iter().map(|s| s.memory_usage).sum::<u64>() / self.samples.len() as u64;
        let avg_sandboxes = self
            .samples
            .iter()
            .map(|s| s.sandbox_count as f64)
            .sum::<f64>()
            / self.samples.len() as f64;

        // Calculate trend
        let trend = if self.samples.len() >= 2 {
            let recent = &self.samples[self.samples.len() - 1];
            let older = &self.samples[self.samples.len() / 2];
            (recent.sandbox_count as f64 - older.sandbox_count as f64)
                / older.sandbox_count.max(1) as f64
        } else {
            0.0
        };

        // Predict with trend
        let predicted_sandboxes = (avg_sandboxes * (1.0 + trend * 0.5)) as u32;
        let recommended = predicted_sandboxes
            .max(self.config.min_pool_size)
            .min(self.config.max_pool_size);

        let confidence = (self.samples.len() as f64 / self.config.sample_window as f64).min(1.0);

        ResourcePrediction {
            cpu_usage: avg_cpu * (1.0 + trend * 0.3),
            memory_usage: avg_mem,
            sandbox_count: predicted_sandboxes,
            confidence,
            horizon: self.config.prediction_horizon,
            recommended_pool_size: recommended,
        }
    }

    /// Get recommended scaling action.
    pub fn recommend_action(&self) -> ScalingAction {
        // Check cooldown
        if let Some(last) = self.last_scaling_action {
            if last.elapsed() < self.config.cooldown {
                return ScalingAction::Maintain;
            }
        }

        let prediction = self.predict();

        if prediction.confidence < 0.5 {
            return ScalingAction::Maintain;
        }

        if prediction.cpu_usage > self.config.scale_up_threshold * 100.0 {
            let needed = prediction
                .recommended_pool_size
                .saturating_sub(self.current_pool_size);
            if needed > 0 {
                return ScalingAction::ScaleUp(needed.min(10));
            }
        }

        if prediction.cpu_usage < self.config.scale_down_threshold * 100.0 {
            let excess = self
                .current_pool_size
                .saturating_sub(prediction.recommended_pool_size);
            if excess > 0 && self.current_pool_size > self.config.min_pool_size {
                return ScalingAction::ScaleDown(excess.min(5));
            }
        }

        ScalingAction::Maintain
    }

    /// Apply a scaling action.
    pub fn apply_action(&mut self, action: ScalingAction) {
        match action {
            ScalingAction::ScaleUp(n) => {
                self.current_pool_size =
                    (self.current_pool_size + n).min(self.config.max_pool_size);
            }
            ScalingAction::ScaleDown(n) => {
                self.current_pool_size = self
                    .current_pool_size
                    .saturating_sub(n)
                    .max(self.config.min_pool_size);
            }
            ScalingAction::Maintain => {}
        }
        self.last_scaling_action = Some(Instant::now());
    }

    /// Get current pool size.
    pub fn current_pool_size(&self) -> u32 {
        self.current_pool_size
    }

    /// Get sample count.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_sample(cpu: f64, sandboxes: u32) -> ResourceSample {
        ResourceSample {
            timestamp: std::time::SystemTime::now(),
            cpu_usage: cpu,
            memory_usage: 1024 * 1024 * 100,
            sandbox_count: sandboxes,
            request_rate: 10.0,
        }
    }

    #[test]
    fn test_scaler_creation() {
        let scaler = PredictiveScaler::new(ScalerConfig::default());
        assert_eq!(scaler.current_pool_size(), 5);
    }

    #[test]
    fn test_record_sample() {
        let mut scaler = PredictiveScaler::new(ScalerConfig::default());
        scaler.record_sample(create_sample(50.0, 10));
        assert_eq!(scaler.sample_count(), 1);
    }

    #[test]
    fn test_prediction() {
        let mut scaler = PredictiveScaler::new(ScalerConfig::default());

        for i in 0..10 {
            scaler.record_sample(create_sample(50.0 + i as f64, 10 + i));
        }

        let prediction = scaler.predict();
        assert!(prediction.confidence > 0.0);
        assert!(prediction.sandbox_count > 0);
    }

    #[test]
    fn test_scaling_action() {
        let config = ScalerConfig {
            cooldown: Duration::ZERO,
            ..Default::default()
        };
        let mut scaler = PredictiveScaler::new(config);

        for _ in 0..60 {
            scaler.record_sample(create_sample(90.0, 20));
        }

        let action = scaler.recommend_action();
        assert!(matches!(action, ScalingAction::ScaleUp(_)));
    }
}
