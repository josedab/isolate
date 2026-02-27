//! Optimization recommendations based on analytics.

use serde::{Deserialize, Serialize};

use super::instrumentation::ExecutionMetrics;

/// Types of recommendations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendationType {
    ReduceMemory,
    IncreaseFuel,
    OptimizeIo,
    EnableCaching,
    ScaleDown,
    InvestigateErrors,
}

/// A generated recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub rec_type: RecommendationType,
    pub title: String,
    pub description: String,
    pub priority: u8, // 1 (highest) to 5 (lowest)
    pub estimated_impact: String,
}

/// Analyzes execution metrics and produces recommendations.
pub struct RecommendationEngine {
    high_memory_threshold_mb: f64,
    high_error_rate_threshold: f64,
    low_fuel_efficiency_threshold: f64,
}

impl RecommendationEngine {
    pub fn new() -> Self {
        Self {
            high_memory_threshold_mb: 64.0,
            high_error_rate_threshold: 0.1,
            low_fuel_efficiency_threshold: 0.5,
        }
    }

    /// Configure memory threshold.
    pub fn with_memory_threshold(mut self, mb: f64) -> Self {
        self.high_memory_threshold_mb = mb;
        self
    }

    /// Configure error rate threshold.
    pub fn with_error_threshold(mut self, rate: f64) -> Self {
        self.high_error_rate_threshold = rate;
        self
    }

    /// Analyze metrics and produce recommendations.
    pub fn analyze(&self, metrics: &[ExecutionMetrics]) -> Vec<Recommendation> {
        if metrics.is_empty() {
            return Vec::new();
        }

        let mut recs = Vec::new();

        self.check_memory_usage(metrics, &mut recs);
        self.check_error_rate(metrics, &mut recs);
        self.check_io_efficiency(metrics, &mut recs);
        self.check_fuel_usage(metrics, &mut recs);

        recs.sort_by_key(|r| r.priority);
        recs
    }

    fn check_memory_usage(&self, metrics: &[ExecutionMetrics], recs: &mut Vec<Recommendation>) {
        let avg_memory_mb =
            metrics.iter().map(|m| m.memory_mb()).sum::<f64>() / metrics.len() as f64;

        if avg_memory_mb > self.high_memory_threshold_mb {
            recs.push(Recommendation {
                rec_type: RecommendationType::ReduceMemory,
                title: "High memory usage detected".into(),
                description: format!(
                    "Average memory usage is {:.1} MB, exceeding the {:.1} MB threshold. \
                     Consider optimizing module memory allocation or lowering limits.",
                    avg_memory_mb, self.high_memory_threshold_mb
                ),
                priority: 2,
                estimated_impact: format!(
                    "Could save ~{:.0} MB per execution",
                    avg_memory_mb - self.high_memory_threshold_mb
                ),
            });
        }
    }

    fn check_error_rate(&self, metrics: &[ExecutionMetrics], recs: &mut Vec<Recommendation>) {
        let errors = metrics.iter().filter(|m| !m.is_success()).count();
        let rate = errors as f64 / metrics.len() as f64;

        if rate > self.high_error_rate_threshold {
            recs.push(Recommendation {
                rec_type: RecommendationType::InvestigateErrors,
                title: "High error rate detected".into(),
                description: format!(
                    "Error rate is {:.1}% ({} failures out of {} executions). \
                     Investigate failing modules for root cause.",
                    rate * 100.0,
                    errors,
                    metrics.len()
                ),
                priority: 1,
                estimated_impact: format!(
                    "Reduce failures by {:.0}%",
                    (rate - self.high_error_rate_threshold) * 100.0
                ),
            });
        }
    }

    fn check_io_efficiency(&self, metrics: &[ExecutionMetrics], recs: &mut Vec<Recommendation>) {
        let high_io_count = metrics
            .iter()
            .filter(|m| m.total_io() > 10 * 1024 * 1024) // >10MB
            .count();

        if high_io_count > metrics.len() / 4 {
            recs.push(Recommendation {
                rec_type: RecommendationType::OptimizeIo,
                title: "Excessive I/O detected".into(),
                description: format!(
                    "{} out of {} executions had >10 MB of I/O. \
                     Consider caching or batching I/O operations.",
                    high_io_count,
                    metrics.len()
                ),
                priority: 3,
                estimated_impact: "Potential 2-5x throughput improvement".into(),
            });
        }
    }

    fn check_fuel_usage(&self, metrics: &[ExecutionMetrics], recs: &mut Vec<Recommendation>) {
        let high_fuel = metrics.iter().filter(|m| m.fuel_consumed > 10_000_000).count();

        let ratio = high_fuel as f64 / metrics.len() as f64;
        if ratio > self.low_fuel_efficiency_threshold {
            recs.push(Recommendation {
                rec_type: RecommendationType::IncreaseFuel,
                title: "High fuel consumption pattern".into(),
                description: format!(
                    "{:.0}% of executions consume >10M fuel units. \
                     Consider profiling hot paths or increasing fuel allocation.",
                    ratio * 100.0
                ),
                priority: 3,
                estimated_impact: "Prevent unexpected fuel exhaustion".into(),
            });
        }
    }
}

impl Default for RecommendationEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metrics(
        memory_bytes: u64,
        exit_code: i32,
        io_read: u64,
        fuel: u64,
    ) -> ExecutionMetrics {
        ExecutionMetrics {
            sandbox_id: "sb-1".into(),
            module_name: "test.wasm".into(),
            duration_us: 5000,
            memory_peak_bytes: memory_bytes,
            fuel_consumed: fuel,
            io_bytes_read: io_read,
            io_bytes_written: 0,
            exit_code,
        }
    }

    #[test]
    fn test_high_memory_recommendation() {
        let engine = RecommendationEngine::new().with_memory_threshold(32.0);
        let metrics: Vec<_> = (0..5)
            .map(|_| make_metrics(64 * 1024 * 1024, 0, 0, 0)) // 64MB
            .collect();
        let recs = engine.analyze(&metrics);
        assert!(recs.iter().any(|r| r.rec_type == RecommendationType::ReduceMemory));
    }

    #[test]
    fn test_high_error_rate_recommendation() {
        let engine = RecommendationEngine::new();
        let mut metrics: Vec<_> = (0..3).map(|_| make_metrics(1024, 1, 0, 0)).collect();
        metrics.extend((0..2).map(|_| make_metrics(1024, 0, 0, 0)));
        let recs = engine.analyze(&metrics);
        assert!(recs.iter().any(|r| r.rec_type == RecommendationType::InvestigateErrors));
    }

    #[test]
    fn test_no_recommendations_for_healthy_system() {
        let engine = RecommendationEngine::new();
        let metrics: Vec<_> = (0..10).map(|_| make_metrics(1024 * 1024, 0, 1024, 1000)).collect();
        let recs = engine.analyze(&metrics);
        assert!(recs.is_empty());
    }

    #[test]
    fn test_empty_metrics() {
        let engine = RecommendationEngine::new();
        assert!(engine.analyze(&[]).is_empty());
    }

    #[test]
    fn test_priority_ordering() {
        let engine = RecommendationEngine::new().with_memory_threshold(1.0);
        // High errors + high memory to trigger multiple recommendations
        let metrics: Vec<_> = (0..5).map(|_| make_metrics(64 * 1024 * 1024, 1, 0, 0)).collect();
        let recs = engine.analyze(&metrics);
        assert!(recs.len() >= 2);
        // Check sorted by priority
        for w in recs.windows(2) {
            assert!(w[0].priority <= w[1].priority);
        }
    }

    #[test]
    fn test_io_recommendation() {
        let engine = RecommendationEngine::new();
        let metrics: Vec<_> = (0..4)
            .map(|_| make_metrics(1024, 0, 20 * 1024 * 1024, 0)) // 20MB I/O
            .collect();
        let recs = engine.analyze(&metrics);
        assert!(recs.iter().any(|r| r.rec_type == RecommendationType::OptimizeIo));
    }
}
