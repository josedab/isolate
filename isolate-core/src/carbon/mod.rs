//! Carbon-Aware Execution
//!
//! Schedule execution based on carbon intensity:
//! - Integration with electricity grid carbon data
//! - Automatic deferral to low-carbon periods
//! - Carbon footprint tracking per sandbox
//! - Regional carbon optimization

pub mod alert;
pub mod cost;
pub mod optimizer;

pub use alert::{Alert, AlertManager, AlertRule, AlertSeverity, AlertTrigger};
pub use cost::{CloudProvider, CostBreakdown, CostEstimator, CostRecord, CostSummary, PricingTier};
pub use optimizer::{
    OptimizationCategory, Recommendation, RecommendationPriority, ResourceOptimizer,
    SuggestedAction, UsagePattern,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Carbon intensity measurement (gCO2eq/kWh).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CarbonIntensity(pub f64);

impl CarbonIntensity {
    /// Create new carbon intensity value.
    pub fn new(gco2_per_kwh: f64) -> Self {
        Self(gco2_per_kwh)
    }

    /// Get value in gCO2eq/kWh.
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Classify intensity level.
    pub fn level(&self) -> IntensityLevel {
        match self.0 {
            x if x < 50.0 => IntensityLevel::VeryLow,
            x if x < 100.0 => IntensityLevel::Low,
            x if x < 200.0 => IntensityLevel::Moderate,
            x if x < 400.0 => IntensityLevel::High,
            _ => IntensityLevel::VeryHigh,
        }
    }
}

/// Carbon intensity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntensityLevel {
    VeryLow,
    Low,
    Moderate,
    High,
    VeryHigh,
}

/// Grid region for carbon data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GridRegion {
    /// Region code.
    pub code: String,
    /// Region name.
    pub name: String,
    /// Timezone.
    pub timezone: String,
}

impl GridRegion {
    /// Create a new region.
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        Self { code: code.into(), name: name.into(), timezone: "UTC".to_string() }
    }
}

/// Carbon data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarbonDataPoint {
    /// Timestamp.
    pub timestamp: SystemTime,
    /// Carbon intensity.
    pub intensity: CarbonIntensity,
    /// Region.
    pub region: GridRegion,
    /// Data source.
    pub source: String,
}

/// Forecast for carbon intensity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarbonForecast {
    /// Region.
    pub region: GridRegion,
    /// Forecast points.
    pub points: Vec<ForecastPoint>,
    /// Generated at.
    pub generated_at: SystemTime,
}

/// A point in the carbon forecast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastPoint {
    /// Timestamp.
    pub timestamp: SystemTime,
    /// Predicted intensity.
    pub intensity: CarbonIntensity,
    /// Confidence (0.0-1.0).
    pub confidence: f64,
}

/// Carbon-aware scheduling decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulingDecision {
    /// Execute immediately.
    ExecuteNow,
    /// Defer to specified time.
    DeferUntil(SystemTime),
    /// Execute in specified region.
    MigrateToRegion(String),
    /// Cannot schedule within constraints.
    CannotSchedule,
}

/// Carbon budget for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarbonBudget {
    /// Maximum carbon allowed (gCO2eq).
    pub max_carbon: f64,
    /// Current carbon used (gCO2eq).
    pub used_carbon: f64,
    /// Budget period.
    pub period: Duration,
    /// Period start.
    pub period_start: SystemTime,
}

impl CarbonBudget {
    /// Create a new carbon budget.
    pub fn new(max_carbon: f64, period: Duration) -> Self {
        Self { max_carbon, used_carbon: 0.0, period, period_start: SystemTime::now() }
    }

    /// Get remaining budget.
    pub fn remaining(&self) -> f64 {
        (self.max_carbon - self.used_carbon).max(0.0)
    }

    /// Check if budget exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.used_carbon >= self.max_carbon
    }

    /// Get usage percentage.
    pub fn usage_percentage(&self) -> f64 {
        if self.max_carbon == 0.0 {
            0.0
        } else {
            (self.used_carbon / self.max_carbon * 100.0).min(100.0)
        }
    }

    /// Add carbon usage.
    pub fn add_usage(&mut self, carbon: f64) {
        self.used_carbon += carbon;
    }

    /// Reset budget for new period.
    pub fn reset(&mut self) {
        self.used_carbon = 0.0;
        self.period_start = SystemTime::now();
    }
}

/// Carbon footprint for an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarbonFootprint {
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Carbon emitted (gCO2eq).
    pub carbon_grams: f64,
    /// Energy consumed (kWh).
    pub energy_kwh: f64,
    /// Execution duration.
    pub duration: Duration,
    /// Region.
    pub region: GridRegion,
    /// Timestamp.
    pub timestamp: SystemTime,
}

/// Configuration for carbon-aware scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarbonSchedulerConfig {
    /// Maximum intensity for immediate execution.
    pub max_immediate_intensity: CarbonIntensity,
    /// Maximum deferral time.
    pub max_deferral: Duration,
    /// Enable region migration.
    pub enable_migration: bool,
    /// Available regions.
    pub regions: Vec<GridRegion>,
    /// Power consumption per sandbox (kW).
    pub power_per_sandbox: f64,
}

impl Default for CarbonSchedulerConfig {
    fn default() -> Self {
        Self {
            max_immediate_intensity: CarbonIntensity::new(200.0),
            max_deferral: Duration::from_secs(3600 * 4), // 4 hours
            enable_migration: false,
            regions: vec![GridRegion::new("default", "Default Region")],
            power_per_sandbox: 0.001, // 1 Watt
        }
    }
}

/// Carbon-aware sandbox scheduler.
pub struct CarbonScheduler {
    config: CarbonSchedulerConfig,
    current_intensities: HashMap<String, CarbonDataPoint>,
    forecasts: HashMap<String, CarbonForecast>,
    footprints: Vec<CarbonFootprint>,
    budgets: HashMap<String, CarbonBudget>,
}

impl Default for CarbonScheduler {
    fn default() -> Self {
        Self::new(CarbonSchedulerConfig::default())
    }
}

impl CarbonScheduler {
    /// Create a new carbon scheduler.
    pub fn new(config: CarbonSchedulerConfig) -> Self {
        Self {
            config,
            current_intensities: HashMap::new(),
            forecasts: HashMap::new(),
            footprints: Vec::new(),
            budgets: HashMap::new(),
        }
    }

    /// Update carbon intensity for a region.
    pub fn update_intensity(&mut self, data: CarbonDataPoint) {
        self.current_intensities.insert(data.region.code.clone(), data);
    }

    /// Update forecast for a region.
    pub fn update_forecast(&mut self, forecast: CarbonForecast) {
        self.forecasts.insert(forecast.region.code.clone(), forecast);
    }

    /// Get current intensity for region.
    pub fn get_intensity(&self, region: &str) -> Option<&CarbonDataPoint> {
        self.current_intensities.get(region)
    }

    /// Make scheduling decision.
    pub fn schedule(&self, region: &str, deadline: Option<SystemTime>) -> SchedulingDecision {
        let current = match self.current_intensities.get(region) {
            Some(data) => data.intensity,
            None => return SchedulingDecision::ExecuteNow, // No data, execute now
        };

        // If current intensity is acceptable, execute now
        if current.0 <= self.config.max_immediate_intensity.0 {
            return SchedulingDecision::ExecuteNow;
        }

        // Check forecast for low-carbon window
        if let Some(forecast) = self.forecasts.get(region) {
            let now = SystemTime::now();
            let max_time = deadline.unwrap_or_else(|| now + self.config.max_deferral);

            for point in &forecast.points {
                if point.timestamp > now && point.timestamp <= max_time {
                    if point.intensity.0 <= self.config.max_immediate_intensity.0 {
                        return SchedulingDecision::DeferUntil(point.timestamp);
                    }
                }
            }
        }

        // Check if migration is possible
        if self.config.enable_migration {
            for (region_code, data) in &self.current_intensities {
                if region_code != region
                    && data.intensity.0 <= self.config.max_immediate_intensity.0
                {
                    return SchedulingDecision::MigrateToRegion(region_code.clone());
                }
            }
        }

        // No good option found, check deadline
        if deadline.is_some() {
            SchedulingDecision::ExecuteNow // Must execute before deadline
        } else {
            SchedulingDecision::CannotSchedule
        }
    }

    /// Record carbon footprint for an execution.
    pub fn record_footprint(&mut self, sandbox_id: &str, duration: Duration, region: &str) {
        let intensity =
            self.current_intensities.get(region).map(|d| d.intensity.0).unwrap_or(100.0);

        let energy_kwh = self.config.power_per_sandbox * duration.as_secs_f64() / 3600.0;
        let carbon_grams = energy_kwh * intensity;

        let footprint = CarbonFootprint {
            sandbox_id: sandbox_id.to_string(),
            carbon_grams,
            energy_kwh,
            duration,
            region: GridRegion::new(region, region),
            timestamp: SystemTime::now(),
        };

        self.footprints.push(footprint);

        // Update budget if exists
        if let Some(budget) = self.budgets.get_mut(sandbox_id) {
            budget.add_usage(carbon_grams);
        }
    }

    /// Set carbon budget for sandbox.
    pub fn set_budget(&mut self, sandbox_id: &str, budget: CarbonBudget) {
        self.budgets.insert(sandbox_id.to_string(), budget);
    }

    /// Get budget for sandbox.
    pub fn get_budget(&self, sandbox_id: &str) -> Option<&CarbonBudget> {
        self.budgets.get(sandbox_id)
    }

    /// Get total carbon footprint.
    pub fn total_footprint(&self) -> f64 {
        self.footprints.iter().map(|f| f.carbon_grams).sum()
    }

    /// Get footprint by sandbox.
    pub fn footprint_by_sandbox(&self, sandbox_id: &str) -> f64 {
        self.footprints.iter().filter(|f| f.sandbox_id == sandbox_id).map(|f| f.carbon_grams).sum()
    }

    /// Get carbon statistics.
    pub fn stats(&self) -> CarbonStats {
        let total_carbon = self.total_footprint();
        let total_energy: f64 = self.footprints.iter().map(|f| f.energy_kwh).sum();
        let avg_intensity = if total_energy > 0.0 { total_carbon / total_energy } else { 0.0 };

        CarbonStats {
            total_carbon_grams: total_carbon,
            total_energy_kwh: total_energy,
            average_intensity: CarbonIntensity::new(avg_intensity),
            execution_count: self.footprints.len(),
        }
    }

    /// Find optimal execution window.
    pub fn find_optimal_window(
        &self,
        region: &str,
        duration: Duration,
        within: Duration,
    ) -> Option<SystemTime> {
        let forecast = self.forecasts.get(region)?;
        let now = SystemTime::now();
        let deadline = now + within;

        forecast
            .points
            .iter()
            .filter(|p| p.timestamp >= now && p.timestamp + duration <= deadline)
            .min_by(|a, b| a.intensity.0.partial_cmp(&b.intensity.0).unwrap())
            .map(|p| p.timestamp)
    }
}

/// Carbon statistics.
#[derive(Debug, Clone, Default)]
pub struct CarbonStats {
    /// Total carbon emitted (gCO2eq).
    pub total_carbon_grams: f64,
    /// Total energy consumed (kWh).
    pub total_energy_kwh: f64,
    /// Average carbon intensity.
    pub average_intensity: CarbonIntensity,
    /// Number of executions.
    pub execution_count: usize,
}

impl Default for CarbonIntensity {
    fn default() -> Self {
        Self(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_carbon_intensity() {
        let low = CarbonIntensity::new(30.0);
        assert_eq!(low.level(), IntensityLevel::VeryLow);

        let high = CarbonIntensity::new(350.0);
        assert_eq!(high.level(), IntensityLevel::High);
    }

    #[test]
    fn test_grid_region() {
        let region = GridRegion::new("US-CAL", "California");
        assert_eq!(region.code, "US-CAL");
        assert_eq!(region.name, "California");
    }

    #[test]
    fn test_carbon_budget() {
        let mut budget = CarbonBudget::new(100.0, Duration::from_secs(3600));
        assert_eq!(budget.remaining(), 100.0);
        assert!(!budget.is_exceeded());

        budget.add_usage(50.0);
        assert_eq!(budget.remaining(), 50.0);
        assert_eq!(budget.usage_percentage(), 50.0);

        budget.add_usage(60.0);
        assert!(budget.is_exceeded());
    }

    #[test]
    fn test_scheduler_creation() {
        let scheduler = CarbonScheduler::default();
        assert_eq!(scheduler.total_footprint(), 0.0);
    }

    #[test]
    fn test_update_intensity() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(150.0),
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        let data = scheduler.get_intensity("US-CAL");
        assert!(data.is_some());
        assert_eq!(data.unwrap().intensity.0, 150.0);
    }

    #[test]
    fn test_schedule_low_intensity() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(50.0), // Low
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        let decision = scheduler.schedule("US-CAL", None);
        assert_eq!(decision, SchedulingDecision::ExecuteNow);
    }

    #[test]
    fn test_schedule_high_intensity_no_forecast() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(500.0), // Very high
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        let decision = scheduler.schedule("US-CAL", None);
        assert_eq!(decision, SchedulingDecision::CannotSchedule);
    }

    #[test]
    fn test_record_footprint() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(100.0),
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        scheduler.record_footprint("sandbox-1", Duration::from_secs(3600), "US-CAL");

        let footprint = scheduler.footprint_by_sandbox("sandbox-1");
        assert!(footprint > 0.0);
    }

    #[test]
    fn test_carbon_stats() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(100.0),
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        scheduler.record_footprint("sandbox-1", Duration::from_secs(3600), "US-CAL");
        scheduler.record_footprint("sandbox-2", Duration::from_secs(1800), "US-CAL");

        let stats = scheduler.stats();
        assert_eq!(stats.execution_count, 2);
        assert!(stats.total_carbon_grams > 0.0);
    }

    #[test]
    fn test_budget_tracking() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.set_budget("sandbox-1", CarbonBudget::new(10.0, Duration::from_secs(3600)));

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(100.0),
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        scheduler.record_footprint("sandbox-1", Duration::from_secs(3600), "US-CAL");

        let budget = scheduler.get_budget("sandbox-1").unwrap();
        assert!(budget.used_carbon > 0.0);
    }

    #[test]
    fn test_intensity_levels() {
        assert_eq!(CarbonIntensity::new(10.0).level(), IntensityLevel::VeryLow);
        assert_eq!(CarbonIntensity::new(75.0).level(), IntensityLevel::Low);
        assert_eq!(CarbonIntensity::new(150.0).level(), IntensityLevel::Moderate);
        assert_eq!(CarbonIntensity::new(300.0).level(), IntensityLevel::High);
        assert_eq!(CarbonIntensity::new(500.0).level(), IntensityLevel::VeryHigh);
    }

    #[test]
    fn test_schedule_with_deadline() {
        let mut scheduler = CarbonScheduler::default();

        scheduler.update_intensity(CarbonDataPoint {
            timestamp: SystemTime::now(),
            intensity: CarbonIntensity::new(500.0),
            region: GridRegion::new("US-CAL", "California"),
            source: "test".to_string(),
        });

        let deadline = SystemTime::now() + Duration::from_secs(60);
        let decision = scheduler.schedule("US-CAL", Some(deadline));
        // With deadline and no good window, must execute now
        assert_eq!(decision, SchedulingDecision::ExecuteNow);
    }

    #[test]
    fn test_find_optimal_window() {
        let mut scheduler = CarbonScheduler::default();
        let region = GridRegion::new("US-CAL", "California");

        let now = SystemTime::now();
        scheduler.update_forecast(CarbonForecast {
            region: region.clone(),
            points: vec![
                ForecastPoint {
                    timestamp: now + Duration::from_secs(3600),
                    intensity: CarbonIntensity::new(200.0),
                    confidence: 0.9,
                },
                ForecastPoint {
                    timestamp: now + Duration::from_secs(7200),
                    intensity: CarbonIntensity::new(50.0),
                    confidence: 0.8,
                },
            ],
            generated_at: now,
        });

        let window = scheduler.find_optimal_window(
            "US-CAL",
            Duration::from_secs(1800),
            Duration::from_secs(10800),
        );

        assert!(window.is_some());
    }
}
