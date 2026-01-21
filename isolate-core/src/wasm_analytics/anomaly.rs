//! Anomaly detection for execution metrics.

use serde::{Deserialize, Serialize};

use super::timeseries::MetricPoint;

/// Types of detectable anomalies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyType {
    Spike,
    Degradation,
    HighErrorRate,
    MemoryLeak,
    ResourceExhaustion,
}

/// Severity classification.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AnomalySeverity {
    Info,
    Warning,
    Critical,
}

/// A detected anomaly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub anomaly_type: AnomalyType,
    pub severity: AnomalySeverity,
    pub metric_name: String,
    pub message: String,
    pub value: f64,
    pub threshold: f64,
    pub timestamp: u64,
}

/// Statistical anomaly detector using z-score method.
#[derive(Clone)]
pub struct AnomalyDetector {
    z_threshold: f64,
}

impl AnomalyDetector {
    /// Create with a z-score threshold (e.g., 2.0 = ~95%, 3.0 = ~99.7%).
    pub fn new(z_threshold: f64) -> Self {
        Self { z_threshold: z_threshold.abs() }
    }

    /// Detect anomalies in time-series data using z-score method.
    pub fn detect(&self, points: &[MetricPoint]) -> Vec<Anomaly> {
        if points.len() < 3 {
            return Vec::new();
        }

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let stddev = variance.sqrt();

        if stddev < f64::EPSILON {
            return Vec::new();
        }

        let threshold = mean + self.z_threshold * stddev;
        let mut anomalies = Vec::new();

        for point in points {
            let z = (point.value - mean) / stddev;
            if z.abs() > self.z_threshold {
                let (anomaly_type, severity) = if z > 0.0 {
                    (AnomalyType::Spike, if z > self.z_threshold * 1.5 {
                        AnomalySeverity::Critical
                    } else {
                        AnomalySeverity::Warning
                    })
                } else {
                    (AnomalyType::Degradation, AnomalySeverity::Info)
                };

                anomalies.push(Anomaly {
                    anomaly_type,
                    severity,
                    metric_name: String::new(),
                    message: format!(
                        "Value {:.2} deviates {:.2}σ from mean {:.2}",
                        point.value, z, mean
                    ),
                    value: point.value,
                    threshold,
                    timestamp: point.timestamp,
                });
            }
        }

        anomalies
    }

    /// Detect monotonically increasing sequences (potential memory leaks).
    pub fn detect_trend(&self, points: &[MetricPoint]) -> Option<Anomaly> {
        if points.len() < 5 {
            return None;
        }

        let mut increasing = 0;
        for w in points.windows(2) {
            if w[1].value >= w[0].value {
                increasing += 1;
            }
        }

        let ratio = increasing as f64 / (points.len() - 1) as f64;
        if ratio > 0.9 {
            let first = points.first().unwrap().value;
            let last = points.last().unwrap().value;
            let growth = if first > 0.0 { (last - first) / first * 100.0 } else { 0.0 };

            Some(Anomaly {
                anomaly_type: AnomalyType::MemoryLeak,
                severity: if growth > 200.0 {
                    AnomalySeverity::Critical
                } else {
                    AnomalySeverity::Warning
                },
                metric_name: String::new(),
                message: format!(
                    "Monotonic increase detected: {:.1}% growth over {} points",
                    growth, points.len()
                ),
                value: last,
                threshold: first * 2.0,
                timestamp: points.last().unwrap().timestamp,
            })
        } else {
            None
        }
    }
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self::new(2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(values: &[f64]) -> Vec<MetricPoint> {
        values.iter().enumerate().map(|(i, &v)| MetricPoint {
            value: v,
            timestamp: i as u64 * 1000,
        }).collect()
    }

    #[test]
    fn test_spike_detection() {
        // Normal values around 10, with a spike at 100
        let data = points(&[10.0, 11.0, 9.0, 10.5, 10.0, 100.0, 10.0]);
        let detector = AnomalyDetector::new(2.0);
        let anomalies = detector.detect(&data);
        assert!(!anomalies.is_empty());
        assert!(anomalies.iter().any(|a| a.anomaly_type == AnomalyType::Spike));
    }

    #[test]
    fn test_no_anomalies_in_stable_data() {
        let data = points(&[10.0, 10.1, 9.9, 10.0, 10.05, 9.95]);
        let detector = AnomalyDetector::new(3.0);
        let anomalies = detector.detect(&data);
        assert!(anomalies.is_empty());
    }

    #[test]
    fn test_insufficient_data() {
        let data = points(&[10.0, 20.0]);
        let detector = AnomalyDetector::new(2.0);
        assert!(detector.detect(&data).is_empty());
    }

    #[test]
    fn test_trend_detection() {
        let data = points(&[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0]);
        let detector = AnomalyDetector::new(2.0);
        let trend = detector.detect_trend(&data);
        assert!(trend.is_some());
        assert_eq!(trend.unwrap().anomaly_type, AnomalyType::MemoryLeak);
    }

    #[test]
    fn test_no_trend_in_fluctuating_data() {
        let data = points(&[10.0, 5.0, 15.0, 8.0, 12.0, 6.0, 14.0]);
        let detector = AnomalyDetector::new(2.0);
        assert!(detector.detect_trend(&data).is_none());
    }

    #[test]
    fn test_severity_escalation() {
        // Extreme spike should be detected as an anomaly
        let data = points(&[1.0, 1.0, 1.0, 1.0, 1.0, 1000.0]);
        let detector = AnomalyDetector::new(2.0);
        let anomalies = detector.detect(&data);
        assert!(!anomalies.is_empty());
        assert!(anomalies.iter().any(|a| a.anomaly_type == AnomalyType::Spike));
    }

    #[test]
    fn test_constant_data() {
        let data = points(&[5.0, 5.0, 5.0, 5.0, 5.0]);
        let detector = AnomalyDetector::new(2.0);
        assert!(detector.detect(&data).is_empty());
    }
}
