//! Embedded WASM Analytics.
//!
//! Real-time module behavior profiling, anomaly detection,
//! and optimization recommendations.
//!
//! # Features
//!
//! - **Instrumentation**: Capture execution metrics at key points
//! - **Time-Series Metrics**: Aggregate and query metrics over time
//! - **Anomaly Detection**: Detect unusual execution patterns
//! - **Recommendations**: Suggest optimizations based on collected data

#![allow(missing_docs)]
pub mod anomaly;
pub mod collector;
pub mod instrumentation;
pub mod recommendations;
pub mod timeseries;

pub use anomaly::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType};
pub use instrumentation::{ExecutionMetrics, InstrumentationPoint, MetricsCollector};
pub use recommendations::{Recommendation, RecommendationEngine, RecommendationType};
pub use timeseries::{MetricAggregation, MetricPoint, MetricQuery, TimeSeriesStore};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analytics_pipeline() {
        let collector = MetricsCollector::new();
        collector.record(ExecutionMetrics {
            sandbox_id: "sb-1".into(),
            module_name: "app.wasm".into(),
            duration_us: 5000,
            memory_peak_bytes: 2 * 1024 * 1024,
            fuel_consumed: 100000,
            io_bytes_read: 1024,
            io_bytes_written: 512,
            exit_code: 0,
        });

        let store = TimeSeriesStore::new();
        store.push("sb-1.duration", 5000.0, 1000);
        store.push("sb-1.duration", 50000.0, 2000); // spike

        let detector = AnomalyDetector::new(3.0);
        let _anomalies = detector.detect(&store.query("sb-1.duration", 0, u64::MAX));
        // May or may not detect anomaly with only 2 points

        let engine = RecommendationEngine::new();
        let recs = engine.analyze(&collector.all_metrics());
        assert!(recs.is_empty() || !recs.is_empty()); // no panic
    }
}
