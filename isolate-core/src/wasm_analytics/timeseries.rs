//! Time-series metrics storage and querying.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// A single metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub value: f64,
    pub timestamp: u64,
}

/// Query parameters for time-series data.
#[derive(Debug, Clone)]
pub struct MetricQuery {
    pub metric_name: String,
    pub start: u64,
    pub end: u64,
}

/// Aggregation result for a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAggregation {
    pub count: usize,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

impl MetricAggregation {
    /// Compute aggregation from a set of values.
    pub fn from_values(values: &[f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        let stddev = variance.sqrt();

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let percentile = |p: f64| -> f64 {
            let idx = ((p / 100.0) * (count as f64 - 1.0)).round() as usize;
            sorted[idx.min(count - 1)]
        };

        Some(Self {
            count,
            sum,
            min,
            max,
            mean,
            stddev,
            p50: percentile(50.0),
            p95: percentile(95.0),
            p99: percentile(99.0),
        })
    }
}

/// In-memory time-series metric store.
#[derive(Clone)]
pub struct TimeSeriesStore {
    inner: Arc<RwLock<HashMap<String, Vec<MetricPoint>>>>,
}

impl TimeSeriesStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Push a new data point.
    pub fn push(&self, metric_name: &str, value: f64, timestamp: u64) {
        self.inner.write()
            .entry(metric_name.to_string())
            .or_default()
            .push(MetricPoint { value, timestamp });
    }

    /// Query points in a time range.
    pub fn query(&self, metric_name: &str, start: u64, end: u64) -> Vec<MetricPoint> {
        self.inner.read()
            .get(metric_name)
            .map(|points| {
                points.iter()
                    .filter(|p| p.timestamp >= start && p.timestamp <= end)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Aggregate metrics over a time range.
    pub fn aggregate(&self, metric_name: &str, start: u64, end: u64) -> Option<MetricAggregation> {
        let points = self.query(metric_name, start, end);
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        MetricAggregation::from_values(&values)
    }

    /// List all metric names.
    pub fn metric_names(&self) -> Vec<String> {
        self.inner.read().keys().cloned().collect()
    }

    /// Total points stored across all metrics.
    pub fn total_points(&self) -> usize {
        self.inner.read().values().map(|v| v.len()).sum()
    }

    /// Clear all data for a metric.
    pub fn clear_metric(&self, metric_name: &str) {
        self.inner.write().remove(metric_name);
    }

    /// Clear all data.
    pub fn clear_all(&self) {
        self.inner.write().clear();
    }
}

impl Default for TimeSeriesStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_query() {
        let store = TimeSeriesStore::new();
        store.push("cpu", 50.0, 100);
        store.push("cpu", 60.0, 200);
        store.push("cpu", 70.0, 300);

        let points = store.query("cpu", 100, 200);
        assert_eq!(points.len(), 2);
    }

    #[test]
    fn test_aggregation() {
        let store = TimeSeriesStore::new();
        for i in 1..=100 {
            store.push("latency", i as f64, i as u64);
        }
        let agg = store.aggregate("latency", 0, 200).unwrap();
        assert_eq!(agg.count, 100);
        assert!((agg.mean - 50.5).abs() < 0.01);
        assert!((agg.min - 1.0).abs() < 0.01);
        assert!((agg.max - 100.0).abs() < 0.01);
        assert!((agg.p50 - 50.0).abs() < 2.0);
        assert!((agg.p95 - 95.0).abs() < 2.0);
    }

    #[test]
    fn test_empty_query() {
        let store = TimeSeriesStore::new();
        assert!(store.query("unknown", 0, 100).is_empty());
        assert!(store.aggregate("unknown", 0, 100).is_none());
    }

    #[test]
    fn test_metric_names() {
        let store = TimeSeriesStore::new();
        store.push("cpu", 10.0, 1);
        store.push("mem", 20.0, 1);
        let names = store.metric_names();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_clear_metric() {
        let store = TimeSeriesStore::new();
        store.push("cpu", 10.0, 1);
        store.push("mem", 20.0, 1);
        store.clear_metric("cpu");
        assert_eq!(store.total_points(), 1);
    }

    #[test]
    fn test_aggregation_from_values() {
        let agg = MetricAggregation::from_values(&[10.0, 20.0, 30.0]).unwrap();
        assert_eq!(agg.count, 3);
        assert!((agg.mean - 20.0).abs() < 0.01);
        assert!(MetricAggregation::from_values(&[]).is_none());
    }
}
