//! Machine learning model abstraction for anomaly detection.

use super::{AnomalyScore, FeatureVector, ThreatClassification};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for anomaly detection model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier.
    pub model_id: String,
    /// Version of the model.
    pub version: String,
    /// Feature names expected by the model.
    pub feature_names: Vec<String>,
    /// Decision threshold for anomaly classification.
    pub threshold: f64,
    /// Whether to use ensemble of models.
    pub use_ensemble: bool,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_id: "isolate-guardian-v1".to_string(),
            version: "1.0.0".to_string(),
            feature_names: default_feature_names(),
            threshold: 0.7,
            use_ensemble: true,
        }
    }
}

fn default_feature_names() -> Vec<String> {
    vec![
        "instructions_per_second".to_string(),
        "cpu_utilization".to_string(),
        "memory_alloc_rate".to_string(),
        "io_read_rate".to_string(),
        "io_write_rate".to_string(),
        "outbound_connections".to_string(),
        "bytes_sent".to_string(),
        "wasi_calls_per_second".to_string(),
        "math_operation_ratio".to_string(),
        "hash_operation_count".to_string(),
    ]
}

/// Result of model prediction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResult {
    /// Anomaly score (0-1).
    pub score: AnomalyScore,
    /// Threat classification.
    pub classification: ThreatClassification,
    /// Confidence in the classification.
    pub confidence: f64,
    /// Contribution of each feature to the score.
    pub feature_importance: HashMap<String, f64>,
    /// Explanation of the prediction.
    pub explanation: String,
}

impl PredictionResult {
    /// Create a benign prediction result.
    pub fn benign() -> Self {
        Self {
            score: AnomalyScore::new(0.0),
            classification: ThreatClassification::Benign,
            confidence: 1.0,
            feature_importance: HashMap::new(),
            explanation: "No anomalies detected".to_string(),
        }
    }
}

/// Anomaly detection model using ensemble methods.
pub struct AnomalyModel {
    /// Model configuration.
    config: ModelConfig,
    /// Isolation Forest model weights.
    isolation_forest: IsolationForest,
    /// One-Class SVM parameters.
    one_class_svm: OneClassSVM,
    /// Autoencoder reconstruction threshold.
    autoencoder: Autoencoder,
    /// Known malware signatures.
    signatures: SignatureDatabase,
}

impl AnomalyModel {
    /// Create a new anomaly model with default configuration.
    pub fn new() -> Self {
        Self::with_config(ModelConfig::default())
    }

    /// Create a model with custom configuration.
    pub fn with_config(config: ModelConfig) -> Self {
        Self {
            config,
            isolation_forest: IsolationForest::new(),
            one_class_svm: OneClassSVM::new(),
            autoencoder: Autoencoder::new(),
            signatures: SignatureDatabase::new(),
        }
    }

    /// Train the model on a set of samples.
    pub fn train(&mut self, samples: &[FeatureVector]) -> Result<(), ModelError> {
        if samples.is_empty() {
            return Err(ModelError::InvalidFeatures(
                "No samples provided".to_string(),
            ));
        }

        // Extract feature arrays for training
        let feature_refs: Vec<&str> = self
            .config
            .feature_names
            .iter()
            .map(|s| s.as_str())
            .collect();
        let feature_arrays: Vec<Vec<f64>> = samples
            .iter()
            .map(|fv| fv.to_array(&feature_refs))
            .collect();

        // Train each model component (simplified training)
        self.isolation_forest.fit(&feature_arrays);
        self.one_class_svm.fit(&feature_arrays);
        self.autoencoder.fit(&feature_arrays);

        Ok(())
    }

    /// Load a pre-trained model from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ModelError> {
        // In production, this would deserialize a trained model
        let config: ModelConfig =
            serde_json::from_slice(bytes).map_err(|e| ModelError::LoadFailed(e.to_string()))?;
        Ok(Self::with_config(config))
    }

    /// Predict anomaly score for a feature vector.
    pub fn predict(&self, features: &FeatureVector) -> PredictionResult {
        // Convert features to array
        let feature_refs: Vec<&str> = self
            .config
            .feature_names
            .iter()
            .map(|s| s.as_str())
            .collect();
        let feature_array = features.to_array(&feature_refs);

        // Run ensemble predictions
        let isolation_score = self.isolation_forest.score(&feature_array);
        let svm_score = self.one_class_svm.score(&feature_array);
        let autoencoder_score = self.autoencoder.reconstruction_error(&feature_array);

        // Check signature matches
        let signature_match = self.signatures.match_features(features);

        // Combine scores (weighted ensemble)
        let combined_score = if self.config.use_ensemble {
            let weights = [0.35, 0.30, 0.25, 0.10]; // IF, SVM, AE, signatures
            weights[0] * isolation_score
                + weights[1] * svm_score
                + weights[2] * autoencoder_score
                + weights[3] * signature_match.score
        } else {
            isolation_score
        };

        let anomaly_score = AnomalyScore::new(combined_score);

        // Classify the threat
        let (classification, confidence) =
            self.classify_threat(features, anomaly_score, &signature_match);

        // Calculate feature importance
        let feature_importance = self.calculate_importance(features, &feature_array);

        // Generate explanation
        let explanation = self.generate_explanation(&classification, &feature_importance);

        PredictionResult {
            score: anomaly_score,
            classification,
            confidence,
            feature_importance,
            explanation,
        }
    }

    fn classify_threat(
        &self,
        features: &FeatureVector,
        score: AnomalyScore,
        signature_match: &SignatureMatch,
    ) -> (ThreatClassification, f64) {
        // Check signature match first
        if let Some(ref threat_type) = signature_match.threat_type {
            return (threat_type.clone(), signature_match.score);
        }

        if score.value() < self.config.threshold {
            return (ThreatClassification::Benign, 1.0 - score.value());
        }

        // Heuristic classification based on features
        let math_ratio = features.get("math_operation_ratio").unwrap_or(0.0);
        let hash_count = features.get("hash_operation_count").unwrap_or(0.0);
        let bytes_sent = features.get("bytes_sent").unwrap_or(0.0);
        let bytes_received = features.get("bytes_received").unwrap_or(0.0);
        let sensitive_access = features.get("sensitive_file_accesses").unwrap_or(0.0);
        let cpu_util = features.get("cpu_utilization").unwrap_or(0.0);

        // Cryptominer detection
        if math_ratio > 0.7 && hash_count > 500.0 && cpu_util > 80.0 {
            return (ThreatClassification::Cryptominer, 0.85);
        }

        // Data exfiltration detection
        if bytes_sent > bytes_received * 5.0 && sensitive_access > 0.0 {
            return (ThreatClassification::DataExfiltration, 0.8);
        }

        // Resource abuse
        if cpu_util > 95.0 {
            return (ThreatClassification::ResourceAbuse, 0.75);
        }

        // Unknown malware
        if score.value() > 0.9 {
            return (ThreatClassification::UnknownMalware, score.value());
        }

        (ThreatClassification::Suspicious, score.value())
    }

    fn calculate_importance(
        &self,
        _features: &FeatureVector,
        feature_array: &[f64],
    ) -> HashMap<String, f64> {
        let mut importance = HashMap::new();

        // Simple importance: deviation from expected mean
        for (i, &value) in feature_array.iter().enumerate() {
            if let Some(name) = self.config.feature_names.get(i) {
                // Higher absolute value = more important
                importance.insert(name.clone(), value.abs() / (1.0 + value.abs()));
            }
        }

        importance
    }

    fn generate_explanation(
        &self,
        classification: &ThreatClassification,
        importance: &HashMap<String, f64>,
    ) -> String {
        let top_features: Vec<_> = {
            let mut sorted: Vec<_> = importance.iter().collect();
            sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
            sorted.into_iter().take(3).collect()
        };

        let features_str = top_features
            .iter()
            .map(|(k, v)| format!("{} ({:.2})", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        match classification {
            ThreatClassification::Benign => {
                "Behavior appears normal. No suspicious patterns detected.".to_string()
            }
            ThreatClassification::Cryptominer => {
                format!(
                    "Cryptomining behavior detected. High mathematical operations and CPU usage. Key indicators: {}",
                    features_str
                )
            }
            ThreatClassification::DataExfiltration => {
                format!(
                    "Potential data exfiltration detected. Unusual outbound data transfer. Key indicators: {}",
                    features_str
                )
            }
            ThreatClassification::DenialOfService => {
                format!(
                    "DoS pattern detected. Excessive resource consumption. Key indicators: {}",
                    features_str
                )
            }
            ThreatClassification::ResourceAbuse => {
                format!(
                    "Resource abuse detected. Abnormal resource utilization. Key indicators: {}",
                    features_str
                )
            }
            ThreatClassification::SandboxEscape => {
                format!(
                    "Sandbox escape attempt detected. Suspicious system interactions. Key indicators: {}",
                    features_str
                )
            }
            _ => {
                format!(
                    "Suspicious behavior detected. Key indicators: {}",
                    features_str
                )
            }
        }
    }
}

impl Default for AnomalyModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for model operations.
#[derive(Debug, Clone)]
pub enum ModelError {
    /// Failed to load model.
    LoadFailed(String),
    /// Invalid feature format.
    InvalidFeatures(String),
    /// Model not trained.
    NotTrained,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::LoadFailed(msg) => write!(f, "Failed to load model: {}", msg),
            ModelError::InvalidFeatures(msg) => write!(f, "Invalid features: {}", msg),
            ModelError::NotTrained => write!(f, "Model has not been trained"),
        }
    }
}

impl std::error::Error for ModelError {}

// Simplified ML model implementations (in production, use actual ML libraries)

/// Simplified Isolation Forest implementation.
struct IsolationForest {
    n_trees: usize,
    sample_size: usize,
}

impl IsolationForest {
    fn new() -> Self {
        Self {
            n_trees: 100,
            sample_size: 256,
        }
    }

    fn fit(&mut self, _samples: &[Vec<f64>]) {
        // In production, this would build isolation trees from samples
        // Simplified: no-op as we use a heuristic approach
    }

    fn score(&self, features: &[f64]) -> f64 {
        // Simplified: compute based on feature deviations
        let mean = features.iter().sum::<f64>() / features.len() as f64;
        let variance: f64 =
            features.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / features.len() as f64;

        // Higher variance = more anomalous
        let normalized_var = (variance / (1.0 + variance)).min(1.0);

        // Add some non-linearity
        1.0 - (-normalized_var * 2.0).exp()
    }
}

/// Simplified One-Class SVM implementation.
struct OneClassSVM {
    nu: f64,
    gamma: f64,
}

impl OneClassSVM {
    fn new() -> Self {
        Self {
            nu: 0.1,
            gamma: 0.1,
        }
    }

    fn fit(&mut self, _samples: &[Vec<f64>]) {
        // In production, this would compute support vectors
        // Simplified: no-op as we use a heuristic approach
    }

    fn score(&self, features: &[f64]) -> f64 {
        // Simplified: RBF-like scoring based on distance from origin
        let distance: f64 = features.iter().map(|x| x.powi(2)).sum::<f64>().sqrt();
        let normalized = (-self.gamma * distance).exp();
        1.0 - normalized
    }
}

/// Simplified Autoencoder implementation.
struct Autoencoder {
    reconstruction_threshold: f64,
}

impl Autoencoder {
    fn new() -> Self {
        Self {
            reconstruction_threshold: 0.5,
        }
    }

    fn fit(&mut self, _samples: &[Vec<f64>]) {
        // In production, this would train neural network weights
        // Simplified: no-op as we use a heuristic approach
    }

    fn reconstruction_error(&self, features: &[f64]) -> f64 {
        // Simplified: mean absolute deviation from expected
        let mean = features.iter().sum::<f64>() / features.len() as f64;
        let mad: f64 =
            features.iter().map(|x| (x - mean).abs()).sum::<f64>() / features.len() as f64;

        (mad / (1.0 + mad)).min(1.0)
    }
}

/// Database of known malware signatures.
struct SignatureDatabase {
    signatures: Vec<MalwareSignature>,
}

impl SignatureDatabase {
    fn new() -> Self {
        Self {
            signatures: vec![
                MalwareSignature::cryptominer_pattern(),
                MalwareSignature::exfiltration_pattern(),
                MalwareSignature::dos_pattern(),
            ],
        }
    }

    fn match_features(&self, features: &FeatureVector) -> SignatureMatch {
        let mut best_match = SignatureMatch {
            score: 0.0,
            threat_type: None,
            signature_id: None,
        };

        for sig in &self.signatures {
            let score = sig.match_score(features);
            if score > best_match.score && score > sig.threshold {
                best_match = SignatureMatch {
                    score,
                    threat_type: Some(sig.threat_type.clone()),
                    signature_id: Some(sig.id.clone()),
                };
            }
        }

        best_match
    }
}

struct MalwareSignature {
    id: String,
    threat_type: ThreatClassification,
    feature_patterns: HashMap<String, (f64, f64)>, // (min, max) expected range
    threshold: f64,
}

impl MalwareSignature {
    fn cryptominer_pattern() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert("math_operation_ratio".to_string(), (0.7, 1.0));
        patterns.insert("hash_operation_count".to_string(), (500.0, f64::MAX));
        patterns.insert("cpu_utilization".to_string(), (80.0, 100.0));
        patterns.insert("repeated_computation_ratio".to_string(), (0.5, 1.0));

        Self {
            id: "SIG_CRYPTOMINER_001".to_string(),
            threat_type: ThreatClassification::Cryptominer,
            feature_patterns: patterns,
            threshold: 0.7,
        }
    }

    fn exfiltration_pattern() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert("bytes_sent".to_string(), (10000.0, f64::MAX));
        patterns.insert("sensitive_file_accesses".to_string(), (1.0, f64::MAX));
        patterns.insert("unique_destinations".to_string(), (1.0, f64::MAX));

        Self {
            id: "SIG_EXFIL_001".to_string(),
            threat_type: ThreatClassification::DataExfiltration,
            feature_patterns: patterns,
            threshold: 0.6,
        }
    }

    fn dos_pattern() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert("outbound_connections".to_string(), (100.0, f64::MAX));
        patterns.insert("wasi_calls_per_second".to_string(), (1000.0, f64::MAX));

        Self {
            id: "SIG_DOS_001".to_string(),
            threat_type: ThreatClassification::DenialOfService,
            feature_patterns: patterns,
            threshold: 0.7,
        }
    }

    fn match_score(&self, features: &FeatureVector) -> f64 {
        let mut matches = 0;
        let mut total = 0;

        for (feature_name, (min, max)) in &self.feature_patterns {
            total += 1;
            if let Some(value) = features.get(feature_name) {
                if value >= *min && value <= *max {
                    matches += 1;
                }
            }
        }

        if total > 0 {
            matches as f64 / total as f64
        } else {
            0.0
        }
    }
}

struct SignatureMatch {
    score: f64,
    threat_type: Option<ThreatClassification>,
    signature_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_default() {
        let config = ModelConfig::default();
        assert_eq!(config.model_id, "isolate-guardian-v1");
        assert_eq!(config.threshold, 0.7);
    }

    #[test]
    fn test_anomaly_model_benign() {
        let model = AnomalyModel::new();
        let features = FeatureVector::new();

        let result = model.predict(&features);
        assert_eq!(result.classification, ThreatClassification::Benign);
    }

    #[test]
    fn test_anomaly_model_cryptominer() {
        let model = AnomalyModel::new();
        let mut features = FeatureVector::new();
        features.set("math_operation_ratio", 0.9);
        features.set("hash_operation_count", 1000.0);
        features.set("cpu_utilization", 95.0);
        features.set("repeated_computation_ratio", 0.8);

        let result = model.predict(&features);
        assert!(
            result.classification == ThreatClassification::Cryptominer
                || result.classification == ThreatClassification::Suspicious
        );
    }

    #[test]
    fn test_prediction_result_benign() {
        let result = PredictionResult::benign();
        assert_eq!(result.classification, ThreatClassification::Benign);
        assert_eq!(result.score.value(), 0.0);
    }

    #[test]
    fn test_isolation_forest() {
        let forest = IsolationForest::new();

        // Normal values should have low score
        let normal = vec![0.1, 0.2, 0.15, 0.1, 0.18];
        let normal_score = forest.score(&normal);

        // Anomalous values (high variance)
        let anomalous = vec![0.1, 5.0, 0.1, 10.0, 0.1];
        let anomalous_score = forest.score(&anomalous);

        assert!(anomalous_score > normal_score);
    }
}
