//! AI-powered anomaly detection for sandbox execution.
//!
//! This module provides machine learning-based behavioral analysis to detect
//! suspicious patterns during sandbox execution, such as cryptominers,
//! data exfiltration attempts, or DoS patterns.
//!
//! # Features
//!
//! - Real-time behavioral analysis
//! - Pattern matching against known malware signatures
//! - Anomaly scoring with configurable thresholds
//! - Automatic response actions (alert, throttle, terminate)
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::ai::{AnomalyDetector, AnomalyConfig, DetectorAction};
//!
//! let detector = AnomalyDetector::new(AnomalyConfig {
//!     sensitivity: 0.8,
//!     actions: vec![DetectorAction::Alert, DetectorAction::Throttle],
//!     ..Default::default()
//! });
//!
//! // Analyze execution behavior
//! let score = detector.analyze(&behavior_sample)?;
//! if score.is_anomalous() {
//!     println!("Anomaly detected: {:?}", score.classification);
//! }
//! ```

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

mod detector;
mod features;
mod model;
mod patterns;
pub mod pipeline;

pub use detector::{AnomalyConfig, AnomalyDetector, DetectionResult, DetectorAction};
pub use features::{BehaviorFeatures, FeatureExtractor, FeatureVector};
pub use model::{AnomalyModel, ModelConfig, PredictionResult};
pub use patterns::{MalwarePattern, PatternMatcher, ThreatCategory};

use serde::{Deserialize, Serialize};

/// Anomaly score representing the likelihood of malicious behavior.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct AnomalyScore(f64);

impl AnomalyScore {
    /// Create a new anomaly score (clamped to 0.0-1.0).
    pub fn new(score: f64) -> Self {
        Self(score.clamp(0.0, 1.0))
    }

    /// Get the raw score value.
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Check if score exceeds a threshold.
    pub fn exceeds(&self, threshold: f64) -> bool {
        self.0 > threshold
    }

    /// Check if this is considered anomalous (default threshold 0.7).
    pub fn is_anomalous(&self) -> bool {
        self.exceeds(0.7)
    }

    /// Get severity level.
    pub fn severity(&self) -> Severity {
        match self.0 {
            s if s < 0.3 => Severity::Low,
            s if s < 0.5 => Severity::Medium,
            s if s < 0.7 => Severity::High,
            _ => Severity::Critical,
        }
    }
}

impl Default for AnomalyScore {
    fn default() -> Self {
        Self(0.0)
    }
}

/// Severity level of detected anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Low severity - informational.
    Low,
    /// Medium severity - worth investigating.
    Medium,
    /// High severity - likely malicious.
    High,
    /// Critical severity - immediate action required.
    Critical,
}

impl Severity {
    /// Get numeric priority (higher = more severe).
    pub fn priority(&self) -> u8 {
        match self {
            Severity::Low => 1,
            Severity::Medium => 2,
            Severity::High => 3,
            Severity::Critical => 4,
        }
    }
}

/// Classification of detected threat.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreatClassification {
    /// No threat detected.
    Benign,
    /// Cryptocurrency mining behavior.
    Cryptominer,
    /// Data exfiltration attempt.
    DataExfiltration,
    /// Denial of service pattern.
    DenialOfService,
    /// Resource abuse.
    ResourceAbuse,
    /// Sandbox escape attempt.
    SandboxEscape,
    /// Unknown malware pattern.
    UnknownMalware,
    /// Suspicious but unclassified.
    Suspicious,
}

impl ThreatClassification {
    /// Check if this classification represents a threat.
    pub fn is_threat(&self) -> bool {
        !matches!(self, ThreatClassification::Benign)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anomaly_score() {
        let score = AnomalyScore::new(0.85);
        assert!(score.is_anomalous());
        assert_eq!(score.severity(), Severity::Critical);
    }

    #[test]
    fn test_anomaly_score_clamping() {
        let score = AnomalyScore::new(1.5);
        assert_eq!(score.value(), 1.0);

        let score = AnomalyScore::new(-0.5);
        assert_eq!(score.value(), 0.0);
    }

    #[test]
    fn test_severity_priority() {
        assert!(Severity::Critical.priority() > Severity::High.priority());
        assert!(Severity::High.priority() > Severity::Medium.priority());
        assert!(Severity::Medium.priority() > Severity::Low.priority());
    }

    #[test]
    fn test_threat_classification() {
        assert!(!ThreatClassification::Benign.is_threat());
        assert!(ThreatClassification::Cryptominer.is_threat());
        assert!(ThreatClassification::DataExfiltration.is_threat());
    }
}
