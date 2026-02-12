//! ML feature engineering pipeline for sandbox anomaly detection.
//!
//! Extracts behavioral features from sandbox execution for ML model scoring,
//! providing real-time anomaly classification and threat assessment.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Feature vector extracted from sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVector {
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Extraction timestamp.
    pub timestamp: SystemTime,
    /// Numerical features for ML model input.
    pub features: Vec<f64>,
    /// Feature names (aligned with features).
    pub feature_names: Vec<String>,
    /// Categorical features.
    pub categorical: HashMap<String, String>,
}

impl FeatureVector {
    /// Get a feature by name.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.feature_names
            .iter()
            .position(|n| n == name)
            .map(|idx| self.features[idx])
    }

    /// Dimension of the feature vector.
    pub fn dim(&self) -> usize {
        self.features.len()
    }
}

/// Behavioral features extracted from sandbox execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BehaviorProfile {
    // CPU features
    pub fuel_consumed: u64,
    pub fuel_rate: f64,
    pub cpu_burst_count: u32,
    pub cpu_burst_max_duration: Duration,

    // Memory features
    pub peak_memory_bytes: usize,
    pub memory_growth_rate: f64,
    pub allocation_count: u64,
    pub deallocation_count: u64,

    // I/O features
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub io_burst_ratio: f64,

    // Network features
    pub network_connections: u32,
    pub unique_destinations: u32,
    pub dns_queries: u32,
    pub bytes_sent_network: u64,
    pub bytes_recv_network: u64,

    // Filesystem features
    pub files_opened: u32,
    pub unique_paths: u32,
    pub path_depth_max: u32,
    pub sensitive_path_access: bool,

    // Execution pattern features
    pub execution_duration: Duration,
    pub function_call_depth_max: u32,
    pub unique_functions_called: u32,
    pub wasi_call_count: u32,
    pub exception_count: u32,
}

impl BehaviorProfile {
    /// Convert to a normalized feature vector for ML input.
    pub fn to_feature_vector(&self, sandbox_id: &str) -> FeatureVector {
        let names = vec![
            "fuel_consumed",
            "fuel_rate",
            "cpu_burst_count",
            "peak_memory_mb",
            "memory_growth_rate",
            "alloc_dealloc_ratio",
            "bytes_read_kb",
            "bytes_written_kb",
            "io_ops_total",
            "io_burst_ratio",
            "network_connections",
            "unique_destinations",
            "dns_queries",
            "network_bytes_total_kb",
            "files_opened",
            "unique_paths",
            "path_depth_max",
            "sensitive_access",
            "exec_duration_ms",
            "call_depth_max",
            "unique_functions",
            "wasi_calls",
            "exceptions",
        ];

        let alloc_total = (self.allocation_count + self.deallocation_count).max(1) as f64;
        let features = vec![
            self.fuel_consumed as f64,
            self.fuel_rate,
            self.cpu_burst_count as f64,
            self.peak_memory_bytes as f64 / (1024.0 * 1024.0),
            self.memory_growth_rate,
            self.allocation_count as f64 / alloc_total,
            self.bytes_read as f64 / 1024.0,
            self.bytes_written as f64 / 1024.0,
            (self.read_ops + self.write_ops) as f64,
            self.io_burst_ratio,
            self.network_connections as f64,
            self.unique_destinations as f64,
            self.dns_queries as f64,
            (self.bytes_sent_network + self.bytes_recv_network) as f64 / 1024.0,
            self.files_opened as f64,
            self.unique_paths as f64,
            self.path_depth_max as f64,
            if self.sensitive_path_access { 1.0 } else { 0.0 },
            self.execution_duration.as_millis() as f64,
            self.function_call_depth_max as f64,
            self.unique_functions_called as f64,
            self.wasi_call_count as f64,
            self.exception_count as f64,
        ];

        FeatureVector {
            sandbox_id: sandbox_id.to_string(),
            timestamp: SystemTime::now(),
            features,
            feature_names: names.into_iter().map(String::from).collect(),
            categorical: HashMap::new(),
        }
    }
}

/// Result from the ML anomaly detection model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrediction {
    /// Anomaly score (0.0 = normal, 1.0 = definitely anomalous).
    pub anomaly_score: f64,
    /// Classification label.
    pub classification: ThreatClass,
    /// Confidence (0.0-1.0).
    pub confidence: f64,
    /// Feature importance (which features contributed most).
    pub feature_importance: Vec<(String, f64)>,
    /// Model version used.
    pub model_version: String,
    /// Inference latency.
    pub inference_latency: Duration,
}

/// Threat classification categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreatClass {
    Benign,
    Cryptominer,
    DataExfiltration,
    DenialOfService,
    PrivilegeEscalation,
    ReconScanning,
    CommandAndControl,
    Unknown,
}

impl ThreatClass {
    /// Minimum recommended action for this threat class.
    pub fn recommended_action(&self) -> &'static str {
        match self {
            Self::Benign => "none",
            Self::Cryptominer => "terminate",
            Self::DataExfiltration => "suspend_and_alert",
            Self::DenialOfService => "throttle",
            Self::PrivilegeEscalation => "terminate_and_alert",
            Self::ReconScanning => "monitor",
            Self::CommandAndControl => "terminate_and_alert",
            Self::Unknown => "monitor",
        }
    }

    /// Severity level (1-10).
    pub fn severity(&self) -> u8 {
        match self {
            Self::Benign => 0,
            Self::ReconScanning => 4,
            Self::DenialOfService => 6,
            Self::Unknown => 5,
            Self::Cryptominer => 7,
            Self::DataExfiltration => 9,
            Self::PrivilegeEscalation => 10,
            Self::CommandAndControl => 10,
        }
    }
}

/// Heuristic-based detection engine (no ML model required).
pub struct HeuristicDetector {
    /// Detection thresholds.
    thresholds: DetectionThresholds,
}

/// Configurable thresholds for heuristic detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionThresholds {
    /// Network connections threshold for scanning detection.
    pub network_scan_threshold: u32,
    /// DNS query threshold for exfiltration detection.
    pub dns_exfil_threshold: u32,
    /// CPU usage ratio for cryptomining detection.
    pub crypto_cpu_ratio: f64,
    /// Memory growth rate for resource abuse.
    pub memory_abuse_rate: f64,
    /// Maximum path depth before suspicious.
    pub suspicious_path_depth: u32,
    /// Anomaly score threshold for alerts.
    pub alert_threshold: f64,
}

impl Default for DetectionThresholds {
    fn default() -> Self {
        Self {
            network_scan_threshold: 50,
            dns_exfil_threshold: 100,
            crypto_cpu_ratio: 0.95,
            memory_abuse_rate: 10.0,
            suspicious_path_depth: 10,
            alert_threshold: 0.7,
        }
    }
}

impl HeuristicDetector {
    /// Create with default thresholds.
    pub fn new() -> Self {
        Self { thresholds: DetectionThresholds::default() }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(thresholds: DetectionThresholds) -> Self {
        Self { thresholds }
    }

    /// Detect threats from a behavior profile.
    pub fn detect(&self, profile: &BehaviorProfile) -> ModelPrediction {
        let start = std::time::Instant::now();

        let mut scores: Vec<(ThreatClass, f64)> = Vec::new();
        let mut importance = Vec::new();

        // Cryptomining: high CPU, low I/O
        let total_io = (profile.bytes_read + profile.bytes_written).max(1);
        let cpu_io_ratio = profile.fuel_consumed as f64 / total_io as f64;
        if cpu_io_ratio > 1000.0 && profile.fuel_consumed > 100_000 {
            let score = (cpu_io_ratio / 10000.0).min(1.0);
            scores.push((ThreatClass::Cryptominer, score));
            importance.push(("fuel_consumed".to_string(), score));
        }

        // Port scanning: many connections to unique destinations
        if profile.unique_destinations > self.thresholds.network_scan_threshold {
            let score =
                (profile.unique_destinations as f64 / self.thresholds.network_scan_threshold as f64)
                    .min(1.0);
            scores.push((ThreatClass::ReconScanning, score));
            importance.push(("unique_destinations".to_string(), score));
        }

        // Data exfiltration: high DNS or network output
        if profile.dns_queries > self.thresholds.dns_exfil_threshold {
            let score = (profile.dns_queries as f64 / self.thresholds.dns_exfil_threshold as f64)
                .min(1.0);
            scores.push((ThreatClass::DataExfiltration, score));
            importance.push(("dns_queries".to_string(), score));
        }

        // Sensitive path access
        if profile.sensitive_path_access
            || profile.path_depth_max > self.thresholds.suspicious_path_depth
        {
            scores.push((ThreatClass::PrivilegeEscalation, 0.6));
            importance.push(("sensitive_access".to_string(), 0.6));
        }

        // Pick highest-scoring threat
        let (classification, anomaly_score) = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or((ThreatClass::Benign, 0.0));

        let confidence = if anomaly_score > 0.8 { 0.9 } else { anomaly_score * 0.8 + 0.1 };

        ModelPrediction {
            anomaly_score,
            classification,
            confidence,
            feature_importance: importance,
            model_version: "heuristic-v1".to_string(),
            inference_latency: start.elapsed(),
        }
    }
}

impl Default for HeuristicDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Real-time detection pipeline processing behavior profiles.
pub struct DetectionPipeline {
    /// Heuristic detector.
    detector: HeuristicDetector,
    /// Recent predictions.
    predictions: Vec<(String, ModelPrediction)>,
    /// Alert threshold.
    alert_threshold: f64,
    /// Maximum stored predictions.
    max_predictions: usize,
}

impl DetectionPipeline {
    /// Create a new pipeline.
    pub fn new(alert_threshold: f64) -> Self {
        Self {
            detector: HeuristicDetector::new(),
            predictions: Vec::new(),
            alert_threshold,
            max_predictions: 1000,
        }
    }

    /// Process a behavior profile and return prediction.
    pub fn process(&mut self, sandbox_id: &str, profile: &BehaviorProfile) -> ModelPrediction {
        let prediction = self.detector.detect(profile);

        if self.predictions.len() >= self.max_predictions {
            self.predictions.remove(0);
        }
        self.predictions.push((sandbox_id.to_string(), prediction.clone()));

        if prediction.anomaly_score >= self.alert_threshold {
            tracing::warn!(
                sandbox_id = sandbox_id,
                score = prediction.anomaly_score,
                class = ?prediction.classification,
                "Anomaly detected"
            );
        }

        prediction
    }

    /// Get recent alerts (above threshold).
    pub fn recent_alerts(&self) -> Vec<(&str, &ModelPrediction)> {
        self.predictions
            .iter()
            .filter(|(_, p)| p.anomaly_score >= self.alert_threshold)
            .map(|(id, p)| (id.as_str(), p))
            .collect()
    }

    /// Get detection statistics.
    pub fn stats(&self) -> PipelineStats {
        let total = self.predictions.len();
        let alerts = self.predictions.iter().filter(|(_, p)| p.anomaly_score >= self.alert_threshold).count();

        let mut class_counts: HashMap<ThreatClass, usize> = HashMap::new();
        for (_, pred) in &self.predictions {
            *class_counts.entry(pred.classification).or_default() += 1;
        }

        PipelineStats {
            total_processed: total,
            alerts_triggered: alerts,
            classification_counts: class_counts,
        }
    }
}

/// Pipeline statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStats {
    pub total_processed: usize,
    pub alerts_triggered: usize,
    pub classification_counts: HashMap<ThreatClass, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn benign_profile() -> BehaviorProfile {
        BehaviorProfile {
            fuel_consumed: 10_000,
            bytes_read: 50_000,
            bytes_written: 10_000,
            read_ops: 100,
            write_ops: 50,
            network_connections: 2,
            unique_destinations: 2,
            dns_queries: 3,
            files_opened: 5,
            unique_paths: 3,
            execution_duration: Duration::from_millis(100),
            ..Default::default()
        }
    }

    fn cryptominer_profile() -> BehaviorProfile {
        BehaviorProfile {
            fuel_consumed: 10_000_000,
            fuel_rate: 100_000.0,
            bytes_read: 100,
            bytes_written: 50,
            network_connections: 1,
            execution_duration: Duration::from_secs(30),
            ..Default::default()
        }
    }

    fn scanner_profile() -> BehaviorProfile {
        BehaviorProfile {
            network_connections: 200,
            unique_destinations: 150,
            dns_queries: 200,
            execution_duration: Duration::from_secs(10),
            ..Default::default()
        }
    }

    #[test]
    fn test_feature_vector_extraction() {
        let profile = benign_profile();
        let fv = profile.to_feature_vector("sb-1");

        assert_eq!(fv.dim(), 23);
        assert!(fv.get("fuel_consumed").is_some());
        assert!(fv.get("nonexistent").is_none());
    }

    #[test]
    fn test_detect_benign() {
        let detector = HeuristicDetector::new();
        let prediction = detector.detect(&benign_profile());

        assert_eq!(prediction.classification, ThreatClass::Benign);
        assert!(prediction.anomaly_score < 0.5);
    }

    #[test]
    fn test_detect_cryptominer() {
        let detector = HeuristicDetector::new();
        let prediction = detector.detect(&cryptominer_profile());

        assert_eq!(prediction.classification, ThreatClass::Cryptominer);
        assert!(prediction.anomaly_score > 0.5);
    }

    #[test]
    fn test_detect_scanner() {
        let detector = HeuristicDetector::new();
        let prediction = detector.detect(&scanner_profile());

        assert!(prediction.anomaly_score > 0.5);
    }

    #[test]
    fn test_threat_class_severity() {
        assert_eq!(ThreatClass::Benign.severity(), 0);
        assert!(ThreatClass::Cryptominer.severity() > ThreatClass::ReconScanning.severity());
        assert_eq!(ThreatClass::PrivilegeEscalation.severity(), 10);
    }

    #[test]
    fn test_detection_pipeline() {
        let mut pipeline = DetectionPipeline::new(0.5);

        let pred = pipeline.process("sb-1", &benign_profile());
        assert_eq!(pred.classification, ThreatClass::Benign);

        let pred = pipeline.process("sb-2", &cryptominer_profile());
        assert_eq!(pred.classification, ThreatClass::Cryptominer);

        let alerts = pipeline.recent_alerts();
        assert!(!alerts.is_empty());
    }

    #[test]
    fn test_pipeline_stats() {
        let mut pipeline = DetectionPipeline::new(0.5);

        pipeline.process("sb-1", &benign_profile());
        pipeline.process("sb-2", &cryptominer_profile());

        let stats = pipeline.stats();
        assert_eq!(stats.total_processed, 2);
    }
}
