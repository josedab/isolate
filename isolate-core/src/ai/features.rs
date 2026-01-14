//! Feature extraction from sandbox execution behavior.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// A vector of extracted features for ML analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureVector {
    /// Named features and their values.
    features: HashMap<String, f64>,
    /// Timestamp when features were extracted.
    pub extracted_at: Option<std::time::SystemTime>,
}

impl FeatureVector {
    /// Create a new empty feature vector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a feature value.
    pub fn set(&mut self, name: impl Into<String>, value: f64) {
        self.features.insert(name.into(), value);
    }

    /// Get a feature value.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.features.get(name).copied()
    }

    /// Get all features as a slice for ML models.
    pub fn to_array(&self, feature_names: &[&str]) -> Vec<f64> {
        feature_names.iter().map(|name| self.features.get(*name).copied().unwrap_or(0.0)).collect()
    }

    /// Normalize features to 0-1 range.
    pub fn normalize(&mut self, mins: &HashMap<String, f64>, maxs: &HashMap<String, f64>) {
        for (name, value) in self.features.iter_mut() {
            if let (Some(&min), Some(&max)) = (mins.get(name), maxs.get(name)) {
                if max > min {
                    *value = (*value - min) / (max - min);
                }
            }
        }
    }

    /// Get the number of features.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Compute cosine similarity with another vector.
    pub fn cosine_similarity(&self, other: &FeatureVector) -> f64 {
        let mut dot = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for (name, &a) in &self.features {
            if let Some(&b) = other.features.get(name) {
                dot += a * b;
            }
            norm_a += a * a;
        }

        for &b in other.features.values() {
            norm_b += b * b;
        }

        if norm_a > 0.0 && norm_b > 0.0 {
            dot / (norm_a.sqrt() * norm_b.sqrt())
        } else {
            0.0
        }
    }
}

/// Raw behavioral features extracted from execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BehaviorFeatures {
    // CPU patterns
    /// Instructions executed per second.
    pub instructions_per_second: f64,
    /// CPU utilization percentage.
    pub cpu_utilization: f64,
    /// Number of tight loops detected.
    pub tight_loop_count: u64,
    /// Average loop iteration count.
    pub avg_loop_iterations: f64,

    // Memory patterns
    /// Memory allocation rate (bytes/sec).
    pub memory_alloc_rate: f64,
    /// Memory access pattern entropy.
    pub memory_access_entropy: f64,
    /// Peak memory usage.
    pub peak_memory: u64,
    /// Number of memory regions accessed.
    pub memory_regions_accessed: u64,
    /// Sequential vs random access ratio.
    pub sequential_access_ratio: f64,

    // I/O patterns
    /// Bytes read per second.
    pub io_read_rate: f64,
    /// Bytes written per second.
    pub io_write_rate: f64,
    /// Number of unique files accessed.
    pub unique_files_accessed: u64,
    /// Sensitive file access attempts.
    pub sensitive_file_accesses: u64,

    // Network patterns
    /// Outbound connection attempts.
    pub outbound_connections: u64,
    /// Unique destination IPs.
    pub unique_destinations: u64,
    /// Data sent (bytes).
    pub bytes_sent: u64,
    /// Data received (bytes).
    pub bytes_received: u64,
    /// DNS query count.
    pub dns_queries: u64,
    /// Connection to known bad IPs.
    pub suspicious_connections: u64,

    // WASI patterns
    /// WASI calls per second.
    pub wasi_calls_per_second: f64,
    /// Unique WASI functions called.
    pub unique_wasi_functions: u64,
    /// Failed WASI call ratio.
    pub wasi_error_ratio: f64,

    // Crypto patterns (potential cryptominer indicators)
    /// Ratio of math-heavy operations.
    pub math_operation_ratio: f64,
    /// SHA/hash-like operation patterns.
    pub hash_operation_count: u64,
    /// Repeated identical computations.
    pub repeated_computation_ratio: f64,

    // Timing patterns
    /// Execution duration.
    pub execution_duration: Duration,
    /// Time between WASI calls variance.
    pub syscall_timing_variance: f64,
    /// Suspicious timing patterns (side channels).
    pub timing_anomalies: u64,
}

impl BehaviorFeatures {
    /// Create new behavior features.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to feature vector for ML.
    pub fn to_feature_vector(&self) -> FeatureVector {
        let mut vec = FeatureVector::new();

        // CPU features
        vec.set("instructions_per_second", self.instructions_per_second);
        vec.set("cpu_utilization", self.cpu_utilization);
        vec.set("tight_loop_count", self.tight_loop_count as f64);
        vec.set("avg_loop_iterations", self.avg_loop_iterations);

        // Memory features
        vec.set("memory_alloc_rate", self.memory_alloc_rate);
        vec.set("memory_access_entropy", self.memory_access_entropy);
        vec.set("peak_memory", self.peak_memory as f64);
        vec.set("memory_regions_accessed", self.memory_regions_accessed as f64);
        vec.set("sequential_access_ratio", self.sequential_access_ratio);

        // I/O features
        vec.set("io_read_rate", self.io_read_rate);
        vec.set("io_write_rate", self.io_write_rate);
        vec.set("unique_files_accessed", self.unique_files_accessed as f64);
        vec.set("sensitive_file_accesses", self.sensitive_file_accesses as f64);

        // Network features
        vec.set("outbound_connections", self.outbound_connections as f64);
        vec.set("unique_destinations", self.unique_destinations as f64);
        vec.set("bytes_sent", self.bytes_sent as f64);
        vec.set("bytes_received", self.bytes_received as f64);
        vec.set("dns_queries", self.dns_queries as f64);
        vec.set("suspicious_connections", self.suspicious_connections as f64);

        // WASI features
        vec.set("wasi_calls_per_second", self.wasi_calls_per_second);
        vec.set("unique_wasi_functions", self.unique_wasi_functions as f64);
        vec.set("wasi_error_ratio", self.wasi_error_ratio);

        // Crypto features
        vec.set("math_operation_ratio", self.math_operation_ratio);
        vec.set("hash_operation_count", self.hash_operation_count as f64);
        vec.set("repeated_computation_ratio", self.repeated_computation_ratio);

        // Timing features
        vec.set("execution_duration_secs", self.execution_duration.as_secs_f64());
        vec.set("syscall_timing_variance", self.syscall_timing_variance);
        vec.set("timing_anomalies", self.timing_anomalies as f64);

        vec.extracted_at = Some(std::time::SystemTime::now());
        vec
    }

    /// Calculate a simple risk score based on heuristics.
    pub fn heuristic_risk_score(&self) -> f64 {
        let mut score: f64 = 0.0;

        // Cryptominer indicators
        if self.math_operation_ratio > 0.8 {
            score += 0.3;
        }
        if self.hash_operation_count > 1000 {
            score += 0.2;
        }
        if self.repeated_computation_ratio > 0.5 {
            score += 0.2;
        }

        // Data exfiltration indicators
        if self.bytes_sent > self.bytes_received * 10 {
            score += 0.3;
        }
        if self.sensitive_file_accesses > 0 {
            score += 0.2;
        }

        // Resource abuse
        if self.cpu_utilization > 95.0 && self.execution_duration > Duration::from_secs(60) {
            score += 0.2;
        }

        // Suspicious network
        if self.suspicious_connections > 0 {
            score += 0.4;
        }

        score.min(1.0)
    }
}

/// Extracts features from execution traces.
pub struct FeatureExtractor {
    /// Window size for rate calculations.
    window_size: Duration,
    /// Sensitive file patterns.
    sensitive_patterns: Vec<String>,
    /// Known suspicious IPs/domains.
    suspicious_destinations: Vec<String>,
}

impl FeatureExtractor {
    /// Create a new feature extractor.
    pub fn new() -> Self {
        Self {
            window_size: Duration::from_secs(1),
            sensitive_patterns: vec![
                "/etc/passwd".to_string(),
                "/etc/shadow".to_string(),
                ".ssh/".to_string(),
                ".aws/".to_string(),
                ".env".to_string(),
                "credentials".to_string(),
                "secret".to_string(),
            ],
            suspicious_destinations: vec![],
        }
    }

    /// Set the analysis window size.
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window_size = window;
        self
    }

    /// Add sensitive file patterns.
    pub fn with_sensitive_patterns(mut self, patterns: Vec<String>) -> Self {
        self.sensitive_patterns.extend(patterns);
        self
    }

    /// Add suspicious destinations.
    pub fn with_suspicious_destinations(mut self, destinations: Vec<String>) -> Self {
        self.suspicious_destinations.extend(destinations);
        self
    }

    /// Extract features from execution events.
    pub fn extract(&self, events: &[ExecutionEvent]) -> BehaviorFeatures {
        let mut features = BehaviorFeatures::new();

        if events.is_empty() {
            return features;
        }

        let duration = self.calculate_duration(events);
        features.execution_duration = duration;

        // Calculate rates
        let duration_secs = duration.as_secs_f64().max(0.001);

        // Count event types
        let mut instruction_count = 0u64;
        let mut wasi_calls = 0u64;
        let mut wasi_errors = 0u64;
        let mut _memory_writes = 0u64;
        let mut bytes_read = 0u64;
        let mut bytes_written = 0u64;
        let mut unique_wasi = std::collections::HashSet::new();

        for event in events {
            match &event.event_type {
                ExecutionEventType::Instruction => instruction_count += 1,
                ExecutionEventType::WasiCall { name, success } => {
                    wasi_calls += 1;
                    unique_wasi.insert(name.clone());
                    if !success {
                        wasi_errors += 1;
                    }
                }
                ExecutionEventType::MemoryWrite { size, .. } => {
                    _memory_writes += 1;
                    bytes_written += *size as u64;
                }
                ExecutionEventType::MemoryRead { size, .. } => {
                    bytes_read += *size as u64;
                }
                ExecutionEventType::FileAccess { path, .. } => {
                    features.unique_files_accessed += 1;
                    if self.is_sensitive_path(path) {
                        features.sensitive_file_accesses += 1;
                    }
                }
                ExecutionEventType::NetworkConnect { destination } => {
                    features.outbound_connections += 1;
                    if self.is_suspicious_destination(destination) {
                        features.suspicious_connections += 1;
                    }
                }
                _ => {}
            }
        }

        features.instructions_per_second = instruction_count as f64 / duration_secs;
        features.wasi_calls_per_second = wasi_calls as f64 / duration_secs;
        features.unique_wasi_functions = unique_wasi.len() as u64;
        features.wasi_error_ratio =
            if wasi_calls > 0 { wasi_errors as f64 / wasi_calls as f64 } else { 0.0 };
        features.io_read_rate = bytes_read as f64 / duration_secs;
        features.io_write_rate = bytes_written as f64 / duration_secs;

        features
    }

    fn calculate_duration(&self, events: &[ExecutionEvent]) -> Duration {
        if events.len() < 2 {
            return Duration::ZERO;
        }
        events
            .last()
            .unwrap()
            .timestamp
            .duration_since(events[0].timestamp)
            .unwrap_or(Duration::ZERO)
    }

    fn is_sensitive_path(&self, path: &str) -> bool {
        let path_lower = path.to_lowercase();
        self.sensitive_patterns.iter().any(|p| path_lower.contains(p))
    }

    fn is_suspicious_destination(&self, destination: &str) -> bool {
        self.suspicious_destinations.iter().any(|d| destination.contains(d))
    }
}

impl Default for FeatureExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// An execution event for feature extraction.
#[derive(Debug, Clone)]
pub struct ExecutionEvent {
    /// Timestamp of the event.
    pub timestamp: std::time::SystemTime,
    /// Type of event.
    pub event_type: ExecutionEventType,
}

/// Types of execution events.
#[derive(Debug, Clone)]
pub enum ExecutionEventType {
    /// Instruction executed.
    Instruction,
    /// WASI call made.
    WasiCall { name: String, success: bool },
    /// Memory read.
    MemoryRead { address: u64, size: u32 },
    /// Memory write.
    MemoryWrite { address: u64, size: u32 },
    /// File access.
    FileAccess { path: String, read: bool, write: bool },
    /// Network connection.
    NetworkConnect { destination: String },
    /// DNS query.
    DnsQuery { domain: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_vector() {
        let mut vec = FeatureVector::new();
        vec.set("cpu", 0.5);
        vec.set("memory", 0.8);

        assert_eq!(vec.get("cpu"), Some(0.5));
        assert_eq!(vec.get("memory"), Some(0.8));
        assert_eq!(vec.get("missing"), None);
    }

    #[test]
    fn test_feature_vector_to_array() {
        let mut vec = FeatureVector::new();
        vec.set("a", 1.0);
        vec.set("b", 2.0);
        vec.set("c", 3.0);

        let arr = vec.to_array(&["a", "c", "missing"]);
        assert_eq!(arr, vec![1.0, 3.0, 0.0]);
    }

    #[test]
    fn test_cosine_similarity() {
        let mut v1 = FeatureVector::new();
        v1.set("a", 1.0);
        v1.set("b", 0.0);

        let mut v2 = FeatureVector::new();
        v2.set("a", 1.0);
        v2.set("b", 0.0);

        assert!((v1.cosine_similarity(&v2) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_behavior_features_to_vector() {
        let features = BehaviorFeatures {
            instructions_per_second: 1000.0,
            cpu_utilization: 50.0,
            ..Default::default()
        };

        let vec = features.to_feature_vector();
        assert_eq!(vec.get("instructions_per_second"), Some(1000.0));
        assert_eq!(vec.get("cpu_utilization"), Some(50.0));
    }

    #[test]
    fn test_heuristic_risk_score() {
        let mut features = BehaviorFeatures::new();
        assert!(features.heuristic_risk_score() < 0.1);

        features.suspicious_connections = 5;
        assert!(features.heuristic_risk_score() >= 0.4);
    }

    #[test]
    fn test_feature_extractor_sensitive_paths() {
        let extractor = FeatureExtractor::new();
        assert!(extractor.is_sensitive_path("/etc/passwd"));
        assert!(extractor.is_sensitive_path("/home/user/.ssh/id_rsa"));
        assert!(!extractor.is_sensitive_path("/home/user/document.txt"));
    }
}
