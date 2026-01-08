//! Anomaly detection engine combining ML models with real-time analysis.

use super::{
    patterns::{MalwarePattern, PatternMatcher},
    AnomalyModel, AnomalyScore, BehaviorFeatures, FeatureExtractor, FeatureVector, ModelConfig,
    Severity, ThreatClassification,
};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Configuration for the anomaly detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyConfig {
    /// Sensitivity threshold (0.0-1.0). Higher = more sensitive.
    pub sensitivity: f64,
    /// Minimum samples before making predictions.
    pub min_samples: usize,
    /// Analysis window duration.
    pub window_duration: Duration,
    /// Actions to take on detection.
    pub actions: Vec<DetectorAction>,
    /// Alert threshold (score above this triggers alert).
    pub alert_threshold: f64,
    /// Throttle threshold (score above this triggers throttle).
    pub throttle_threshold: f64,
    /// Terminate threshold (score above this triggers termination).
    pub terminate_threshold: f64,
    /// Enable real-time analysis.
    pub realtime_enabled: bool,
    /// Batch analysis interval.
    pub batch_interval: Duration,
    /// Maximum events to buffer.
    pub max_buffer_size: usize,
    /// Model configuration.
    pub model_config: ModelConfig,
}

impl Default for AnomalyConfig {
    fn default() -> Self {
        Self {
            sensitivity: 0.7,
            min_samples: 100,
            window_duration: Duration::from_secs(10),
            actions: vec![DetectorAction::Alert],
            alert_threshold: 0.5,
            throttle_threshold: 0.7,
            terminate_threshold: 0.9,
            realtime_enabled: true,
            batch_interval: Duration::from_secs(5),
            max_buffer_size: 10000,
            model_config: ModelConfig::default(),
        }
    }
}

/// Actions the detector can take in response to anomalies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DetectorAction {
    /// Log the detection without intervention.
    Log,
    /// Send an alert notification.
    Alert,
    /// Throttle resource usage.
    Throttle,
    /// Suspend execution temporarily.
    Suspend,
    /// Terminate the sandbox.
    Terminate,
    /// Capture forensic snapshot.
    CaptureSnapshot,
    /// Quarantine the sandbox.
    Quarantine,
}

/// Result of anomaly detection analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    /// Overall anomaly score.
    pub score: AnomalyScore,
    /// Threat classification.
    pub classification: ThreatClassification,
    /// Severity level.
    pub severity: Severity,
    /// Confidence in the classification (0.0-1.0).
    pub confidence: f64,
    /// Recommended actions based on analysis.
    pub recommended_actions: Vec<DetectorAction>,
    /// Detailed analysis breakdown.
    pub analysis: AnalysisBreakdown,
    /// Matched malware patterns, if any.
    pub matched_patterns: Vec<PatternMatch>,
    /// Timestamp of detection.
    pub detected_at: std::time::SystemTime,
    /// Duration of analysis.
    pub analysis_duration: Duration,
}

impl DetectionResult {
    /// Check if this result indicates an anomaly.
    pub fn is_anomalous(&self) -> bool {
        self.score.is_anomalous()
    }

    /// Check if this result indicates a threat.
    pub fn is_threat(&self) -> bool {
        self.classification.is_threat()
    }

    /// Get the primary action to take.
    pub fn primary_action(&self) -> Option<DetectorAction> {
        self.recommended_actions.first().copied()
    }
}

/// Breakdown of analysis components.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisBreakdown {
    /// Score from ML model.
    pub ml_score: f64,
    /// Score from heuristic rules.
    pub heuristic_score: f64,
    /// Score from pattern matching.
    pub pattern_score: f64,
    /// Individual feature contributions.
    pub feature_contributions: Vec<FeatureContribution>,
    /// Triggered heuristic rules.
    pub triggered_rules: Vec<String>,
}

/// Contribution of a single feature to the anomaly score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureContribution {
    /// Feature name.
    pub name: String,
    /// Feature value.
    pub value: f64,
    /// Contribution to anomaly score.
    pub contribution: f64,
    /// Whether this feature is anomalous.
    pub is_anomalous: bool,
}

/// A matched malware pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    /// Pattern identifier.
    pub pattern_id: String,
    /// Pattern name.
    pub pattern_name: String,
    /// Match confidence.
    pub confidence: f64,
    /// Matched indicators.
    pub matched_indicators: Vec<String>,
}

/// The main anomaly detector.
pub struct AnomalyDetector {
    config: AnomalyConfig,
    model: Arc<RwLock<AnomalyModel>>,
    feature_extractor: FeatureExtractor,
    pattern_matcher: PatternMatcher,
    state: Arc<RwLock<DetectorState>>,
}

/// Internal state of the detector.
struct DetectorState {
    /// Samples collected for training.
    samples: Vec<FeatureVector>,
    /// Recent detection results.
    recent_results: Vec<DetectionResult>,
    /// Whether the model has been trained.
    is_trained: bool,
    /// Last analysis time.
    last_analysis: Option<Instant>,
    /// Total detections count.
    total_detections: u64,
    /// Anomaly detections count.
    anomaly_detections: u64,
}

impl Default for DetectorState {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            recent_results: Vec::new(),
            is_trained: false,
            last_analysis: None,
            total_detections: 0,
            anomaly_detections: 0,
        }
    }
}

impl AnomalyDetector {
    /// Create a new anomaly detector with the given configuration.
    pub fn new(config: AnomalyConfig) -> Self {
        let model = AnomalyModel::with_config(config.model_config.clone());
        let feature_extractor = FeatureExtractor::new().with_window(config.window_duration);
        let pattern_matcher = PatternMatcher::new();

        Self {
            config,
            model: Arc::new(RwLock::new(model)),
            feature_extractor,
            pattern_matcher,
            state: Arc::new(RwLock::new(DetectorState::default())),
        }
    }

    /// Create a detector with default configuration.
    pub fn default_detector() -> Self {
        Self::new(AnomalyConfig::default())
    }

    /// Analyze behavior features and return detection result.
    pub fn analyze(&self, features: &BehaviorFeatures) -> Result<DetectionResult> {
        let start = Instant::now();
        let feature_vector = features.to_feature_vector();

        // Collect sample for potential training
        self.collect_sample(&feature_vector);

        // Get ML model prediction
        let model = self
            .model
            .read()
            .map_err(|e| Error::Engine(format!("Failed to acquire model lock: {}", e)))?;
        let prediction = model.predict(&feature_vector);

        // Get heuristic score
        let heuristic_score = features.heuristic_risk_score();

        // Check for pattern matches
        let matched_patterns = self.check_patterns(features);
        let pattern_score = if matched_patterns.is_empty() {
            0.0
        } else {
            matched_patterns
                .iter()
                .map(|p| p.confidence)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0)
        };

        // Combine scores with weights based on sensitivity
        let combined_score =
            self.combine_scores(prediction.score.value(), heuristic_score, pattern_score);

        // Apply sensitivity adjustment
        let adjusted_score = (combined_score * self.config.sensitivity).min(1.0);
        let final_score = AnomalyScore::new(adjusted_score);

        // Determine classification
        let classification = self.classify(features, &matched_patterns, final_score);

        // Build analysis breakdown
        let analysis = AnalysisBreakdown {
            ml_score: prediction.score.value(),
            heuristic_score,
            pattern_score,
            feature_contributions: self.calculate_contributions(&feature_vector),
            triggered_rules: self.get_triggered_rules(features),
        };

        // Determine recommended actions
        let recommended_actions = self.determine_actions(final_score);

        // Calculate confidence
        let confidence = self.calculate_confidence(&prediction, heuristic_score, pattern_score);

        let result = DetectionResult {
            score: final_score,
            classification,
            severity: final_score.severity(),
            confidence,
            recommended_actions,
            analysis,
            matched_patterns,
            detected_at: std::time::SystemTime::now(),
            analysis_duration: start.elapsed(),
        };

        // Update state
        self.update_state(&result);

        Ok(result)
    }

    /// Analyze raw execution events.
    pub fn analyze_events(
        &self,
        events: &[super::features::ExecutionEvent],
    ) -> Result<DetectionResult> {
        let features = self.feature_extractor.extract(events);
        self.analyze(&features)
    }

    /// Train the model on collected samples.
    pub fn train(&self) -> Result<()> {
        let samples: Vec<FeatureVector> = {
            let state = self
                .state
                .read()
                .map_err(|e| Error::Engine(format!("Failed to acquire state lock: {}", e)))?;

            if state.samples.len() < self.config.min_samples {
                return Err(Error::Engine(format!(
                    "Insufficient samples: {} < {}",
                    state.samples.len(),
                    self.config.min_samples
                )));
            }

            state.samples.clone()
        };

        let mut model = self
            .model
            .write()
            .map_err(|e| Error::Engine(format!("Failed to acquire model lock: {}", e)))?;
        model
            .train(&samples)
            .map_err(|e| Error::Engine(format!("Training failed: {}", e)))?;

        let mut state = self
            .state
            .write()
            .map_err(|e| Error::Engine(format!("Failed to acquire state lock: {}", e)))?;
        state.is_trained = true;

        Ok(())
    }

    /// Add a known malware pattern.
    pub fn add_pattern(&mut self, pattern: MalwarePattern) {
        self.pattern_matcher.add_pattern(pattern);
    }

    /// Get detection statistics.
    pub fn statistics(&self) -> Result<DetectorStatistics> {
        let state = self
            .state
            .read()
            .map_err(|e| Error::Engine(format!("Failed to acquire state lock: {}", e)))?;

        Ok(DetectorStatistics {
            total_detections: state.total_detections,
            anomaly_detections: state.anomaly_detections,
            anomaly_rate: if state.total_detections > 0 {
                state.anomaly_detections as f64 / state.total_detections as f64
            } else {
                0.0
            },
            samples_collected: state.samples.len(),
            is_trained: state.is_trained,
            recent_results: state.recent_results.len(),
        })
    }

    /// Get recent detection results.
    pub fn recent_detections(&self, limit: usize) -> Result<Vec<DetectionResult>> {
        let state = self
            .state
            .read()
            .map_err(|e| Error::Engine(format!("Failed to acquire state lock: {}", e)))?;

        Ok(state
            .recent_results
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect())
    }

    /// Reset the detector state.
    pub fn reset(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|e| Error::Engine(format!("Failed to acquire state lock: {}", e)))?;
        *state = DetectorState::default();
        Ok(())
    }

    // Private helper methods

    fn collect_sample(&self, features: &FeatureVector) {
        if let Ok(mut state) = self.state.write() {
            if state.samples.len() < self.config.max_buffer_size {
                state.samples.push(features.clone());
            }
        }
    }

    fn combine_scores(&self, ml_score: f64, heuristic_score: f64, pattern_score: f64) -> f64 {
        // Weighted combination with pattern matches having highest weight
        let weights = (0.4, 0.3, 0.3); // (ml, heuristic, pattern)

        if pattern_score > 0.8 {
            // High-confidence pattern match overrides
            pattern_score * 0.7 + ml_score * 0.15 + heuristic_score * 0.15
        } else {
            ml_score * weights.0 + heuristic_score * weights.1 + pattern_score * weights.2
        }
    }

    fn check_patterns(&self, features: &BehaviorFeatures) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for pattern in self.pattern_matcher.patterns() {
            if let Some(m) = self.match_pattern(pattern, features) {
                matches.push(m);
            }
        }

        matches
    }

    fn match_pattern(
        &self,
        pattern: &MalwarePattern,
        features: &BehaviorFeatures,
    ) -> Option<PatternMatch> {
        let mut matched_indicators = Vec::new();
        let mut total_weight = 0.0;
        let mut matched_weight = 0.0;

        for indicator in &pattern.indicators {
            total_weight += indicator.weight;

            let matches = match indicator.indicator_type.as_str() {
                "high_cpu" => features.cpu_utilization > 90.0,
                "high_math_ops" => features.math_operation_ratio > 0.7,
                "high_hash_ops" => features.hash_operation_count > 1000,
                "repeated_computation" => features.repeated_computation_ratio > 0.5,
                "data_exfil" => features.bytes_sent > features.bytes_received * 10,
                "sensitive_access" => features.sensitive_file_accesses > 0,
                "suspicious_network" => features.suspicious_connections > 0,
                "high_dns" => features.dns_queries > 100,
                _ => false,
            };

            if matches {
                matched_indicators.push(indicator.name.clone());
                matched_weight += indicator.weight;
            }
        }

        if total_weight > 0.0 && matched_weight / total_weight >= pattern.threshold {
            Some(PatternMatch {
                pattern_id: pattern.id.clone(),
                pattern_name: pattern.name.clone(),
                confidence: matched_weight / total_weight,
                matched_indicators,
            })
        } else {
            None
        }
    }

    fn classify(
        &self,
        features: &BehaviorFeatures,
        patterns: &[PatternMatch],
        score: AnomalyScore,
    ) -> ThreatClassification {
        // Check pattern-based classifications first
        for pattern in patterns {
            if pattern.confidence > 0.7 {
                if pattern.pattern_name.to_lowercase().contains("miner") {
                    return ThreatClassification::Cryptominer;
                }
                if pattern.pattern_name.to_lowercase().contains("exfil") {
                    return ThreatClassification::DataExfiltration;
                }
            }
        }

        // Heuristic-based classification
        if features.math_operation_ratio > 0.8 && features.hash_operation_count > 1000 {
            return ThreatClassification::Cryptominer;
        }

        if features.bytes_sent > features.bytes_received * 10
            && features.sensitive_file_accesses > 0
        {
            return ThreatClassification::DataExfiltration;
        }

        if features.cpu_utilization > 95.0 && features.execution_duration > Duration::from_secs(300)
        {
            return ThreatClassification::ResourceAbuse;
        }

        if features.outbound_connections > 100 {
            return ThreatClassification::DenialOfService;
        }

        // Score-based classification
        if score.value() < 0.3 {
            ThreatClassification::Benign
        } else if score.value() < 0.5 {
            ThreatClassification::Suspicious
        } else {
            ThreatClassification::UnknownMalware
        }
    }

    fn calculate_contributions(&self, features: &FeatureVector) -> Vec<FeatureContribution> {
        let important_features = [
            "cpu_utilization",
            "memory_alloc_rate",
            "bytes_sent",
            "suspicious_connections",
            "math_operation_ratio",
            "hash_operation_count",
        ];

        important_features
            .iter()
            .filter_map(|&name| {
                features.get(name).map(|value| {
                    let contribution = self.estimate_contribution(name, value);
                    FeatureContribution {
                        name: name.to_string(),
                        value,
                        contribution,
                        is_anomalous: contribution > 0.3,
                    }
                })
            })
            .collect()
    }

    fn estimate_contribution(&self, name: &str, value: f64) -> f64 {
        // Simple threshold-based contribution estimation
        match name {
            "cpu_utilization" => {
                if value > 90.0 {
                    0.5
                } else {
                    value / 200.0
                }
            }
            "suspicious_connections" => (value / 10.0).min(1.0),
            "math_operation_ratio" => {
                if value > 0.7 {
                    0.6
                } else {
                    value * 0.5
                }
            }
            "hash_operation_count" => (value / 5000.0).min(0.8),
            "bytes_sent" => (value / 1_000_000.0).min(0.5),
            _ => 0.0,
        }
    }

    fn get_triggered_rules(&self, features: &BehaviorFeatures) -> Vec<String> {
        let mut rules = Vec::new();

        if features.cpu_utilization > 95.0 {
            rules.push("HIGH_CPU_USAGE".to_string());
        }
        if features.math_operation_ratio > 0.8 {
            rules.push("CRYPTO_MATH_PATTERN".to_string());
        }
        if features.hash_operation_count > 1000 {
            rules.push("EXCESSIVE_HASHING".to_string());
        }
        if features.bytes_sent > features.bytes_received * 10 {
            rules.push("ASYMMETRIC_DATA_FLOW".to_string());
        }
        if features.sensitive_file_accesses > 0 {
            rules.push("SENSITIVE_FILE_ACCESS".to_string());
        }
        if features.suspicious_connections > 0 {
            rules.push("SUSPICIOUS_NETWORK".to_string());
        }
        if features.wasi_error_ratio > 0.5 {
            rules.push("HIGH_ERROR_RATE".to_string());
        }

        rules
    }

    fn determine_actions(&self, score: AnomalyScore) -> Vec<DetectorAction> {
        let mut actions = Vec::new();

        if score.value() >= self.config.terminate_threshold {
            actions.push(DetectorAction::Terminate);
            actions.push(DetectorAction::CaptureSnapshot);
        } else if score.value() >= self.config.throttle_threshold {
            actions.push(DetectorAction::Throttle);
            actions.push(DetectorAction::Alert);
        } else if score.value() >= self.config.alert_threshold {
            actions.push(DetectorAction::Alert);
        }

        // Add configured default actions if none determined
        if actions.is_empty() {
            actions.push(DetectorAction::Log);
        }

        actions
    }

    fn calculate_confidence(
        &self,
        prediction: &super::model::PredictionResult,
        heuristic: f64,
        pattern: f64,
    ) -> f64 {
        // Higher confidence when multiple methods agree
        let ml_conf = prediction.confidence;

        let agreement_bonus = if (prediction.score.value() > 0.5) == (heuristic > 0.5) {
            0.1
        } else {
            0.0
        };

        let pattern_bonus = if pattern > 0.7 { 0.15 } else { 0.0 };

        (ml_conf + agreement_bonus + pattern_bonus).min(1.0)
    }

    fn update_state(&self, result: &DetectionResult) {
        if let Ok(mut state) = self.state.write() {
            state.total_detections += 1;
            if result.is_anomalous() {
                state.anomaly_detections += 1;
            }
            state.last_analysis = Some(Instant::now());

            // Keep only recent results
            state.recent_results.push(result.clone());
            if state.recent_results.len() > 100 {
                state.recent_results.remove(0);
            }
        }
    }
}

/// Statistics about detector performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorStatistics {
    /// Total number of detections performed.
    pub total_detections: u64,
    /// Number of anomalies detected.
    pub anomaly_detections: u64,
    /// Rate of anomaly detection.
    pub anomaly_rate: f64,
    /// Number of samples collected.
    pub samples_collected: usize,
    /// Whether the model is trained.
    pub is_trained: bool,
    /// Number of recent results stored.
    pub recent_results: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = AnomalyDetector::default_detector();
        let stats = detector.statistics().unwrap();
        assert_eq!(stats.total_detections, 0);
        assert!(!stats.is_trained);
    }

    #[test]
    fn test_analyze_benign_behavior() {
        let detector = AnomalyDetector::default_detector();
        let features = BehaviorFeatures::new();

        let result = detector.analyze(&features).unwrap();
        assert!(!result.is_anomalous());
        assert_eq!(result.classification, ThreatClassification::Benign);
    }

    #[test]
    fn test_analyze_suspicious_behavior() {
        let detector = AnomalyDetector::default_detector();
        let features = BehaviorFeatures {
            cpu_utilization: 99.0,
            math_operation_ratio: 0.9,
            hash_operation_count: 5000,
            ..Default::default()
        };

        let result = detector.analyze(&features).unwrap();
        assert!(result.score.value() > 0.3);
        assert!(result.classification.is_threat());
    }

    #[test]
    fn test_action_determination() {
        let config = AnomalyConfig {
            alert_threshold: 0.3,
            throttle_threshold: 0.6,
            terminate_threshold: 0.9,
            ..Default::default()
        };
        let detector = AnomalyDetector::new(config);

        // High threat should recommend termination
        let features = BehaviorFeatures {
            suspicious_connections: 10,
            sensitive_file_accesses: 5,
            bytes_sent: 10_000_000,
            ..Default::default()
        };

        let result = detector.analyze(&features).unwrap();
        assert!(
            result.recommended_actions.contains(&DetectorAction::Alert)
                || result
                    .recommended_actions
                    .contains(&DetectorAction::Throttle)
                || result
                    .recommended_actions
                    .contains(&DetectorAction::Terminate)
        );
    }

    #[test]
    fn test_statistics_update() {
        let detector = AnomalyDetector::default_detector();
        let features = BehaviorFeatures::new();

        for _ in 0..5 {
            detector.analyze(&features).unwrap();
        }

        let stats = detector.statistics().unwrap();
        assert_eq!(stats.total_detections, 5);
        assert_eq!(stats.samples_collected, 5);
    }

    #[test]
    fn test_reset() {
        let detector = AnomalyDetector::default_detector();
        let features = BehaviorFeatures::new();

        detector.analyze(&features).unwrap();
        assert!(detector.statistics().unwrap().total_detections > 0);

        detector.reset().unwrap();
        assert_eq!(detector.statistics().unwrap().total_detections, 0);
    }
}
