use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single time-series metric sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp_epoch_ms: u64,
    pub requests_per_sec: f64,
    pub pool_utilization: f64,
}

impl MetricSample {
    pub fn new(rps: f64, utilization: f64) -> Self {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        Self { timestamp_epoch_ms: now, requests_per_sec: rps, pool_utilization: utilization }
    }
}

/// Fixed-size circular buffer for time-series metric samples.
///
/// Retains the last N samples for trend analysis and forecasting.
pub struct MetricsRing {
    samples: parking_lot::Mutex<VecDeque<MetricSample>>,
    max_samples: usize,
}

impl MetricsRing {
    pub fn new(max_samples: usize) -> Self {
        Self { samples: parking_lot::Mutex::new(VecDeque::with_capacity(max_samples)), max_samples }
    }

    /// Add a new sample, evicting oldest if at capacity.
    pub fn record(&self, sample: MetricSample) {
        let mut samples = self.samples.lock();
        if samples.len() >= self.max_samples {
            samples.pop_front();
        }
        samples.push_back(sample);
    }

    /// Get all current samples (oldest first).
    pub fn samples(&self) -> Vec<MetricSample> {
        self.samples.lock().iter().cloned().collect()
    }

    /// Number of samples currently stored.
    pub fn len(&self) -> usize {
        self.samples.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.lock().is_empty()
    }

    /// Average RPS over the window.
    pub fn avg_rps(&self) -> f64 {
        let samples = self.samples.lock();
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|s| s.requests_per_sec).sum();
        sum / samples.len() as f64
    }

    /// Peak RPS in the window.
    pub fn peak_rps(&self) -> f64 {
        self.samples.lock().iter().map(|s| s.requests_per_sec).fold(0.0f64, f64::max)
    }

    /// Current (latest) utilization.
    pub fn current_utilization(&self) -> f64 {
        self.samples.lock().back().map(|s| s.pool_utilization).unwrap_or(0.0)
    }

    /// Clear all samples.
    pub fn clear(&self) {
        self.samples.lock().clear();
    }
}

impl Default for MetricsRing {
    fn default() -> Self {
        Self::new(300) // 5 minutes at 1-second intervals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basics() {
        let ring = MetricsRing::new(3);
        assert!(ring.is_empty());

        ring.record(MetricSample::new(10.0, 0.5));
        ring.record(MetricSample::new(20.0, 0.7));
        assert_eq!(ring.len(), 2);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let ring = MetricsRing::new(3);
        ring.record(MetricSample::new(10.0, 0.5));
        ring.record(MetricSample::new(20.0, 0.6));
        ring.record(MetricSample::new(30.0, 0.7));
        ring.record(MetricSample::new(40.0, 0.8));

        assert_eq!(ring.len(), 3);
        let samples = ring.samples();
        // Oldest (10.0) should be evicted
        assert!((samples[0].requests_per_sec - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_avg_rps() {
        let ring = MetricsRing::new(10);
        ring.record(MetricSample::new(10.0, 0.5));
        ring.record(MetricSample::new(20.0, 0.5));
        ring.record(MetricSample::new(30.0, 0.5));
        assert!((ring.avg_rps() - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_peak_rps() {
        let ring = MetricsRing::new(10);
        ring.record(MetricSample::new(10.0, 0.5));
        ring.record(MetricSample::new(50.0, 0.5));
        ring.record(MetricSample::new(30.0, 0.5));
        assert!((ring.peak_rps() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_current_utilization() {
        let ring = MetricsRing::new(10);
        ring.record(MetricSample::new(10.0, 0.3));
        ring.record(MetricSample::new(20.0, 0.8));
        assert!((ring.current_utilization() - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_empty_stats() {
        let ring = MetricsRing::new(10);
        assert_eq!(ring.avg_rps(), 0.0);
        assert_eq!(ring.peak_rps(), 0.0);
        assert_eq!(ring.current_utilization(), 0.0);
    }

    #[test]
    fn test_clear() {
        let ring = MetricsRing::new(10);
        ring.record(MetricSample::new(10.0, 0.5));
        ring.clear();
        assert!(ring.is_empty());
    }
}
