//! Resource Optimization Recommendations
//!
//! Analyzes usage patterns and suggests optimizations for cost and carbon reduction.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Optimization category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationCategory {
    RightSizing,
    Scheduling,
    RegionOptimization,
    IdleDetection,
    Batching,
}

/// Recommendation priority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecommendationPriority {
    Low,
    Medium,
    High,
}

/// Suggested action to take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestedAction {
    ReduceMemory { current_mb: u64, suggested_mb: u64 },
    ReduceTimeout { current_secs: u64, suggested_secs: u64 },
    MigrateRegion { from: String, to: String },
    BatchExecutions { suggested_batch_size: usize },
    TerminateIdle { sandbox_ids: Vec<String> },
}

/// Optimization recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub category: OptimizationCategory,
    pub title: String,
    pub description: String,
    pub estimated_savings_pct: f64,
    pub estimated_savings_usd: f64,
    pub priority: RecommendationPriority,
    pub action: SuggestedAction,
}

/// Usage pattern for analysis.
#[derive(Debug, Clone)]
pub struct UsagePattern {
    pub sandbox_id: String,
    pub avg_memory_bytes: u64,
    pub peak_memory_bytes: u64,
    pub avg_duration: Duration,
    pub execution_count: u64,
    pub idle_time_pct: f64,
}

/// Optimizer that analyzes patterns and generates recommendations.
pub struct ResourceOptimizer {
    patterns: Vec<UsagePattern>,
    cost_per_mb_hour: f64,
}

impl ResourceOptimizer {
    /// Create a new optimizer with default cost rate.
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            cost_per_mb_hour: 0.000005, // ~$0.005 per GB-hour
        }
    }

    /// Add a usage pattern for analysis.
    pub fn add_pattern(&mut self, pattern: UsagePattern) {
        self.patterns.push(pattern);
    }

    /// Analyze all patterns and generate recommendations.
    pub fn analyze(&self) -> Vec<Recommendation> {
        let mut recommendations = Vec::new();
        recommendations.extend(self.right_size_recommendations());
        recommendations.extend(self.idle_recommendations());
        recommendations.extend(self.batch_recommendations());
        recommendations
    }

    /// Detect sandboxes that have been idle beyond the given threshold.
    pub fn detect_idle(&self, threshold_pct: f64) -> Vec<String> {
        self.patterns
            .iter()
            .filter(|p| p.idle_time_pct > threshold_pct)
            .map(|p| p.sandbox_id.clone())
            .collect()
    }

    /// Generate right-sizing recommendations for over-provisioned sandboxes.
    pub fn right_size_recommendations(&self) -> Vec<Recommendation> {
        let mut recs = Vec::new();

        for pattern in &self.patterns {
            let peak_mb = pattern.peak_memory_bytes / (1024 * 1024);
            let avg_mb = pattern.avg_memory_bytes / (1024 * 1024);

            // Recommend if peak usage is less than 50% of allocated (avg as proxy for allocated)
            if avg_mb > 0 && peak_mb > 0 && peak_mb < avg_mb / 2 {
                let suggested_mb = peak_mb * 2; // 2x headroom over peak
                let saved_mb = avg_mb.saturating_sub(suggested_mb);
                let hours = pattern.avg_duration.as_secs_f64() / 3600.0;
                let savings = saved_mb as f64
                    * hours
                    * self.cost_per_mb_hour
                    * pattern.execution_count as f64;
                let savings_pct =
                    if avg_mb > 0 { saved_mb as f64 / avg_mb as f64 * 100.0 } else { 0.0 };

                recs.push(Recommendation {
                    category: OptimizationCategory::RightSizing,
                    title: format!("Right-size sandbox {}", pattern.sandbox_id),
                    description: format!(
                        "Sandbox {} uses peak {}MB but has {}MB allocated. Reduce to {}MB.",
                        pattern.sandbox_id, peak_mb, avg_mb, suggested_mb
                    ),
                    estimated_savings_pct: savings_pct,
                    estimated_savings_usd: savings,
                    priority: if savings_pct > 50.0 {
                        RecommendationPriority::High
                    } else {
                        RecommendationPriority::Medium
                    },
                    action: SuggestedAction::ReduceMemory { current_mb: avg_mb, suggested_mb },
                });
            }
        }

        recs
    }

    /// Calculate total potential savings from all recommendations.
    pub fn total_potential_savings(&self) -> f64 {
        self.analyze().iter().map(|r| r.estimated_savings_usd).sum()
    }

    fn idle_recommendations(&self) -> Vec<Recommendation> {
        let idle_ids = self.detect_idle(0.8);
        if idle_ids.is_empty() {
            return Vec::new();
        }

        let total_idle_savings: f64 = self
            .patterns
            .iter()
            .filter(|p| idle_ids.contains(&p.sandbox_id))
            .map(|p| {
                let mb = p.avg_memory_bytes as f64 / (1024.0 * 1024.0);
                let hours = p.avg_duration.as_secs_f64() / 3600.0;
                mb * hours * self.cost_per_mb_hour * p.idle_time_pct
            })
            .sum();

        vec![Recommendation {
            category: OptimizationCategory::IdleDetection,
            title: format!("Terminate {} idle sandboxes", idle_ids.len()),
            description: format!(
                "Sandboxes {} are idle >80% of the time and should be terminated.",
                idle_ids.join(", ")
            ),
            estimated_savings_pct: 80.0,
            estimated_savings_usd: total_idle_savings,
            priority: RecommendationPriority::High,
            action: SuggestedAction::TerminateIdle { sandbox_ids: idle_ids },
        }]
    }

    fn batch_recommendations(&self) -> Vec<Recommendation> {
        // Recommend batching if there are many short-lived executions
        let short_lived: Vec<&UsagePattern> = self
            .patterns
            .iter()
            .filter(|p| p.avg_duration < Duration::from_secs(1) && p.execution_count > 100)
            .collect();

        if short_lived.len() < 3 {
            return Vec::new();
        }

        let total_executions: u64 = short_lived.iter().map(|p| p.execution_count).sum();
        // Estimate 30% savings from reduced cold-start overhead
        let savings = total_executions as f64 * 0.001 * 0.30;

        vec![Recommendation {
            category: OptimizationCategory::Batching,
            title: "Batch short-lived executions".to_string(),
            description: format!(
                "{} sandboxes have short-lived executions (< 1s). Batching could reduce overhead.",
                short_lived.len()
            ),
            estimated_savings_pct: 30.0,
            estimated_savings_usd: savings,
            priority: RecommendationPriority::Medium,
            action: SuggestedAction::BatchExecutions { suggested_batch_size: 10 },
        }]
    }
}

impl Default for ResourceOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pattern(id: &str, avg_mb: u64, peak_mb: u64, idle_pct: f64) -> UsagePattern {
        UsagePattern {
            sandbox_id: id.to_string(),
            avg_memory_bytes: avg_mb * 1024 * 1024,
            peak_memory_bytes: peak_mb * 1024 * 1024,
            avg_duration: Duration::from_secs(60),
            execution_count: 100,
            idle_time_pct: idle_pct,
        }
    }

    #[test]
    fn test_right_size_over_provisioned() {
        let mut opt = ResourceOptimizer::new();
        // Peak is 100MB but avg (allocated) is 512MB -> peak < avg/2
        opt.add_pattern(make_pattern("sb-1", 512, 100, 0.1));
        let recs = opt.right_size_recommendations();
        assert_eq!(recs.len(), 1);
        if let SuggestedAction::ReduceMemory { current_mb, suggested_mb } = &recs[0].action {
            assert_eq!(*current_mb, 512);
            assert_eq!(*suggested_mb, 200); // 2x peak
        } else {
            panic!("Expected ReduceMemory action");
        }
    }

    #[test]
    fn test_right_size_well_sized_no_recommendation() {
        let mut opt = ResourceOptimizer::new();
        // Peak is 400MB, avg is 512MB -> peak NOT < avg/2 -> no recommendation
        opt.add_pattern(make_pattern("sb-1", 512, 400, 0.1));
        let recs = opt.right_size_recommendations();
        assert!(recs.is_empty());
    }

    #[test]
    fn test_detect_idle() {
        let mut opt = ResourceOptimizer::new();
        opt.add_pattern(make_pattern("sb-idle", 256, 128, 0.9));
        opt.add_pattern(make_pattern("sb-active", 256, 128, 0.2));
        let idle = opt.detect_idle(0.8);
        assert_eq!(idle, vec!["sb-idle"]);
    }

    #[test]
    fn test_idle_recommendations() {
        let mut opt = ResourceOptimizer::new();
        opt.add_pattern(make_pattern("sb-idle-1", 256, 128, 0.95));
        let recs = opt.analyze();
        let idle_recs: Vec<_> = recs
            .iter()
            .filter(|r| matches!(r.category, OptimizationCategory::IdleDetection))
            .collect();
        assert_eq!(idle_recs.len(), 1);
    }

    #[test]
    fn test_analyze_empty_patterns() {
        let opt = ResourceOptimizer::new();
        let recs = opt.analyze();
        assert!(recs.is_empty());
    }

    #[test]
    fn test_total_potential_savings() {
        let mut opt = ResourceOptimizer::new();
        opt.add_pattern(make_pattern("sb-idle", 256, 128, 0.95));
        let savings = opt.total_potential_savings();
        assert!(savings >= 0.0);
    }

    #[test]
    fn test_batch_recommendations_short_lived() {
        let mut opt = ResourceOptimizer::new();
        for i in 0..5 {
            opt.add_pattern(UsagePattern {
                sandbox_id: format!("sb-short-{}", i),
                avg_memory_bytes: 64 * 1024 * 1024,
                peak_memory_bytes: 32 * 1024 * 1024,
                avg_duration: Duration::from_millis(500),
                execution_count: 500,
                idle_time_pct: 0.1,
            });
        }
        let recs = opt.analyze();
        let batch_recs: Vec<_> =
            recs.iter().filter(|r| matches!(r.category, OptimizationCategory::Batching)).collect();
        assert_eq!(batch_recs.len(), 1);
    }

    #[test]
    fn test_no_batch_for_long_running() {
        let mut opt = ResourceOptimizer::new();
        for i in 0..5 {
            opt.add_pattern(UsagePattern {
                sandbox_id: format!("sb-long-{}", i),
                avg_memory_bytes: 64 * 1024 * 1024,
                peak_memory_bytes: 32 * 1024 * 1024,
                avg_duration: Duration::from_secs(60),
                execution_count: 500,
                idle_time_pct: 0.1,
            });
        }
        let recs = opt.analyze();
        let batch_recs: Vec<_> =
            recs.iter().filter(|r| matches!(r.category, OptimizationCategory::Batching)).collect();
        assert!(batch_recs.is_empty());
    }

    #[test]
    fn test_multiple_recommendation_types() {
        let mut opt = ResourceOptimizer::new();
        // Over-provisioned sandbox
        opt.add_pattern(make_pattern("sb-over", 1024, 100, 0.1));
        // Idle sandbox
        opt.add_pattern(make_pattern("sb-idle", 256, 128, 0.95));
        let recs = opt.analyze();
        let categories: Vec<_> = recs.iter().map(|r| &r.category).collect();
        assert!(categories.iter().any(|c| matches!(c, OptimizationCategory::RightSizing)));
        assert!(categories.iter().any(|c| matches!(c, OptimizationCategory::IdleDetection)));
    }
}
