//! Malware pattern matching and signature database.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Category of malware threat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreatCategory {
    /// Cryptocurrency mining malware.
    Cryptominer,
    /// Ransomware.
    Ransomware,
    /// Data exfiltration/stealer.
    Stealer,
    /// Botnet client.
    Botnet,
    /// Denial of service tool.
    DoS,
    /// Remote access trojan.
    RAT,
    /// Worm/self-propagating.
    Worm,
    /// Privilege escalation.
    PrivEsc,
    /// Resource hijacking.
    ResourceHijack,
    /// Generic malware.
    Generic,
}

impl ThreatCategory {
    /// Get the severity weight for this category.
    pub fn severity_weight(&self) -> f64 {
        match self {
            ThreatCategory::Ransomware => 1.0,
            ThreatCategory::RAT => 0.95,
            ThreatCategory::PrivEsc => 0.9,
            ThreatCategory::Stealer => 0.85,
            ThreatCategory::Botnet => 0.8,
            ThreatCategory::Worm => 0.75,
            ThreatCategory::DoS => 0.7,
            ThreatCategory::Cryptominer => 0.6,
            ThreatCategory::ResourceHijack => 0.5,
            ThreatCategory::Generic => 0.4,
        }
    }

    /// Get all categories.
    pub fn all() -> &'static [ThreatCategory] {
        &[
            ThreatCategory::Cryptominer,
            ThreatCategory::Ransomware,
            ThreatCategory::Stealer,
            ThreatCategory::Botnet,
            ThreatCategory::DoS,
            ThreatCategory::RAT,
            ThreatCategory::Worm,
            ThreatCategory::PrivEsc,
            ThreatCategory::ResourceHijack,
            ThreatCategory::Generic,
        ]
    }
}

/// A behavioral indicator for pattern matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorIndicator {
    /// Indicator name.
    pub name: String,
    /// Type of indicator.
    pub indicator_type: String,
    /// Description.
    pub description: String,
    /// Weight in pattern matching (0.0-1.0).
    pub weight: f64,
    /// Whether this indicator alone is sufficient.
    pub is_atomic: bool,
}

impl BehaviorIndicator {
    /// Create a new behavior indicator.
    pub fn new(name: impl Into<String>, indicator_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            indicator_type: indicator_type.into(),
            description: String::new(),
            weight: 1.0,
            is_atomic: false,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the weight.
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Mark as atomic (sufficient on its own).
    pub fn atomic(mut self) -> Self {
        self.is_atomic = true;
        self
    }
}

/// A malware signature pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalwarePattern {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the malware behavior.
    pub description: String,
    /// Threat category.
    pub category: ThreatCategory,
    /// Behavioral indicators.
    pub indicators: Vec<BehaviorIndicator>,
    /// Minimum match threshold (0.0-1.0).
    pub threshold: f64,
    /// Confidence score of this pattern.
    pub confidence: f64,
    /// Tags for filtering.
    pub tags: Vec<String>,
    /// Whether this pattern is enabled.
    pub enabled: bool,
    /// References (CVE, reports, etc.).
    pub references: Vec<String>,
}

impl MalwarePattern {
    /// Create a new malware pattern.
    pub fn new(id: impl Into<String>, name: impl Into<String>, category: ThreatCategory) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            category,
            indicators: Vec::new(),
            threshold: 0.7,
            confidence: 0.8,
            tags: Vec::new(),
            enabled: true,
            references: Vec::new(),
        }
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add an indicator.
    pub fn with_indicator(mut self, indicator: BehaviorIndicator) -> Self {
        self.indicators.push(indicator);
        self
    }

    /// Set the match threshold.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the confidence score.
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Add tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Add a reference.
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.references.push(reference.into());
        self
    }

    /// Disable this pattern.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Pattern matcher for malware detection.
#[derive(Debug, Clone)]
pub struct PatternMatcher {
    patterns: Vec<MalwarePattern>,
    patterns_by_category: HashMap<ThreatCategory, Vec<usize>>,
    patterns_by_tag: HashMap<String, Vec<usize>>,
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternMatcher {
    /// Create a new pattern matcher.
    pub fn new() -> Self {
        let mut matcher = Self {
            patterns: Vec::new(),
            patterns_by_category: HashMap::new(),
            patterns_by_tag: HashMap::new(),
        };

        // Add built-in patterns
        matcher.add_builtin_patterns();
        matcher
    }

    /// Create an empty pattern matcher without built-in patterns.
    pub fn empty() -> Self {
        Self {
            patterns: Vec::new(),
            patterns_by_category: HashMap::new(),
            patterns_by_tag: HashMap::new(),
        }
    }

    /// Add a pattern to the matcher.
    pub fn add_pattern(&mut self, pattern: MalwarePattern) {
        let index = self.patterns.len();

        // Index by category
        self.patterns_by_category.entry(pattern.category).or_default().push(index);

        // Index by tags
        for tag in &pattern.tags {
            self.patterns_by_tag.entry(tag.clone()).or_default().push(index);
        }

        self.patterns.push(pattern);
    }

    /// Get all patterns.
    pub fn patterns(&self) -> &[MalwarePattern] {
        &self.patterns
    }

    /// Get patterns by category.
    pub fn patterns_by_category(&self, category: ThreatCategory) -> Vec<&MalwarePattern> {
        self.patterns_by_category
            .get(&category)
            .map(|indices| indices.iter().map(|&i| &self.patterns[i]).collect())
            .unwrap_or_default()
    }

    /// Get patterns by tag.
    pub fn patterns_by_tag(&self, tag: &str) -> Vec<&MalwarePattern> {
        self.patterns_by_tag
            .get(tag)
            .map(|indices| indices.iter().map(|&i| &self.patterns[i]).collect())
            .unwrap_or_default()
    }

    /// Get enabled patterns only.
    pub fn enabled_patterns(&self) -> impl Iterator<Item = &MalwarePattern> {
        self.patterns.iter().filter(|p| p.enabled)
    }

    /// Remove a pattern by ID.
    pub fn remove_pattern(&mut self, id: &str) -> Option<MalwarePattern> {
        if let Some(index) = self.patterns.iter().position(|p| p.id == id) {
            let pattern = self.patterns.remove(index);
            self.rebuild_indices();
            Some(pattern)
        } else {
            None
        }
    }

    /// Enable/disable a pattern.
    pub fn set_pattern_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(pattern) = self.patterns.iter_mut().find(|p| p.id == id) {
            pattern.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get pattern count.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Rebuild internal indices after modification.
    fn rebuild_indices(&mut self) {
        self.patterns_by_category.clear();
        self.patterns_by_tag.clear();

        for (index, pattern) in self.patterns.iter().enumerate() {
            self.patterns_by_category.entry(pattern.category).or_default().push(index);

            for tag in &pattern.tags {
                self.patterns_by_tag.entry(tag.clone()).or_default().push(index);
            }
        }
    }

    /// Add built-in malware patterns.
    fn add_builtin_patterns(&mut self) {
        // Cryptominer patterns
        self.add_pattern(
            MalwarePattern::new(
                "miner_001",
                "Generic Cryptominer",
                ThreatCategory::Cryptominer,
            )
            .with_description(
                "Detects cryptocurrency mining behavior based on CPU and math operation patterns",
            )
            .with_indicator(
                BehaviorIndicator::new("high_cpu", "high_cpu")
                    .with_description("Sustained high CPU utilization")
                    .with_weight(0.8),
            )
            .with_indicator(
                BehaviorIndicator::new("high_math", "high_math_ops")
                    .with_description("High ratio of math operations")
                    .with_weight(0.9),
            )
            .with_indicator(
                BehaviorIndicator::new("hash_ops", "high_hash_ops")
                    .with_description("Excessive hash-like operations")
                    .with_weight(0.95),
            )
            .with_indicator(
                BehaviorIndicator::new("repeated_compute", "repeated_computation")
                    .with_description("Repeated identical computations")
                    .with_weight(0.7),
            )
            .with_threshold(0.6)
            .with_tags(vec!["crypto".to_string(), "resource-abuse".to_string()]),
        );

        self.add_pattern(
            MalwarePattern::new("miner_002", "Coinhive-style Miner", ThreatCategory::Cryptominer)
                .with_description("Detects browser-style cryptocurrency mining patterns")
                .with_indicator(
                    BehaviorIndicator::new("sustained_cpu", "high_cpu").with_weight(0.7),
                )
                .with_indicator(
                    BehaviorIndicator::new("network_beacon", "suspicious_network")
                        .with_description("Regular network beaconing")
                        .with_weight(0.6),
                )
                .with_indicator(
                    BehaviorIndicator::new("wasm_compute", "high_math_ops").with_weight(0.8),
                )
                .with_threshold(0.65)
                .with_tags(vec!["crypto".to_string(), "browser".to_string()]),
        );

        // Data exfiltration patterns
        self.add_pattern(
            MalwarePattern::new("exfil_001", "Data Exfiltration", ThreatCategory::Stealer)
                .with_description(
                    "Detects data exfiltration based on network and file access patterns",
                )
                .with_indicator(
                    BehaviorIndicator::new("sensitive_access", "sensitive_access")
                        .with_description("Access to sensitive files")
                        .with_weight(0.9),
                )
                .with_indicator(
                    BehaviorIndicator::new("data_out", "data_exfil")
                        .with_description("Asymmetric data flow (more sent than received)")
                        .with_weight(0.85),
                )
                .with_indicator(
                    BehaviorIndicator::new("bulk_read", "high_io_read")
                        .with_description("High file read activity")
                        .with_weight(0.6),
                )
                .with_threshold(0.7)
                .with_tags(vec!["exfil".to_string(), "data-theft".to_string()]),
        );

        self.add_pattern(
            MalwarePattern::new("exfil_002", "Credential Theft", ThreatCategory::Stealer)
                .with_description("Detects credential harvesting behavior")
                .with_indicator(
                    BehaviorIndicator::new("cred_files", "sensitive_access")
                        .with_description("Access to credential storage files")
                        .with_weight(0.95)
                        .atomic(),
                )
                .with_indicator(
                    BehaviorIndicator::new("network_send", "data_exfil").with_weight(0.7),
                )
                .with_threshold(0.5)
                .with_tags(vec!["credentials".to_string(), "stealer".to_string()]),
        );

        // DoS patterns
        self.add_pattern(
            MalwarePattern::new("dos_001", "Network Flood", ThreatCategory::DoS)
                .with_description("Detects network-based denial of service patterns")
                .with_indicator(
                    BehaviorIndicator::new("conn_flood", "high_connections")
                        .with_description("Excessive outbound connections")
                        .with_weight(0.9),
                )
                .with_indicator(
                    BehaviorIndicator::new("dns_flood", "high_dns")
                        .with_description("Excessive DNS queries")
                        .with_weight(0.7),
                )
                .with_indicator(
                    BehaviorIndicator::new("bandwidth", "high_bandwidth")
                        .with_description("High network bandwidth usage")
                        .with_weight(0.8),
                )
                .with_threshold(0.65)
                .with_tags(vec!["dos".to_string(), "network".to_string()]),
        );

        // Resource abuse patterns
        self.add_pattern(
            MalwarePattern::new(
                "resource_001",
                "Resource Exhaustion",
                ThreatCategory::ResourceHijack,
            )
            .with_description("Detects resource exhaustion attacks")
            .with_indicator(
                BehaviorIndicator::new("mem_exhaust", "high_memory")
                    .with_description("Memory exhaustion attempt")
                    .with_weight(0.85),
            )
            .with_indicator(BehaviorIndicator::new("cpu_exhaust", "high_cpu").with_weight(0.8))
            .with_indicator(
                BehaviorIndicator::new("io_exhaust", "high_io")
                    .with_description("I/O exhaustion")
                    .with_weight(0.7),
            )
            .with_threshold(0.6)
            .with_tags(vec!["resource-abuse".to_string()]),
        );

        // Suspicious network patterns
        self.add_pattern(
            MalwarePattern::new("c2_001", "Command and Control", ThreatCategory::Botnet)
                .with_description("Detects command and control communication patterns")
                .with_indicator(
                    BehaviorIndicator::new("bad_ip", "suspicious_network")
                        .with_description("Connection to known malicious IP")
                        .with_weight(1.0)
                        .atomic(),
                )
                .with_indicator(
                    BehaviorIndicator::new("beacon", "periodic_network")
                        .with_description("Periodic network beaconing")
                        .with_weight(0.7),
                )
                .with_indicator(
                    BehaviorIndicator::new("encoded_traffic", "suspicious_traffic")
                        .with_description("Encoded or encrypted traffic patterns")
                        .with_weight(0.6),
                )
                .with_threshold(0.5)
                .with_tags(vec!["c2".to_string(), "botnet".to_string()]),
        );

        // Generic suspicious patterns
        self.add_pattern(
            MalwarePattern::new("generic_001", "Suspicious Behavior", ThreatCategory::Generic)
                .with_description("Generic suspicious behavior pattern")
                .with_indicator(
                    BehaviorIndicator::new("high_error_rate", "high_errors")
                        .with_description("High WASI error rate")
                        .with_weight(0.5),
                )
                .with_indicator(
                    BehaviorIndicator::new("timing_anomaly", "timing_anomaly")
                        .with_description("Suspicious timing patterns")
                        .with_weight(0.4),
                )
                .with_indicator(
                    BehaviorIndicator::new("unusual_syscalls", "unusual_wasi")
                        .with_description("Unusual WASI call patterns")
                        .with_weight(0.5),
                )
                .with_threshold(0.7)
                .with_tags(vec!["generic".to_string()]),
        );
    }
}

/// Pattern match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatchResult {
    /// Pattern that matched.
    pub pattern_id: String,
    /// Pattern name.
    pub pattern_name: String,
    /// Threat category.
    pub category: ThreatCategory,
    /// Match score (0.0-1.0).
    pub score: f64,
    /// Individual indicator matches.
    pub indicator_matches: Vec<IndicatorMatch>,
    /// Whether this is a high-confidence match.
    pub is_high_confidence: bool,
}

/// Individual indicator match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorMatch {
    /// Indicator name.
    pub name: String,
    /// Whether it matched.
    pub matched: bool,
    /// Contribution to overall score.
    pub contribution: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threat_category_severity() {
        assert!(
            ThreatCategory::Ransomware.severity_weight()
                > ThreatCategory::Cryptominer.severity_weight()
        );
        assert!(ThreatCategory::RAT.severity_weight() > ThreatCategory::DoS.severity_weight());
    }

    #[test]
    fn test_behavior_indicator() {
        let indicator = BehaviorIndicator::new("test", "test_type")
            .with_description("Test indicator")
            .with_weight(0.8)
            .atomic();

        assert_eq!(indicator.name, "test");
        assert_eq!(indicator.weight, 0.8);
        assert!(indicator.is_atomic);
    }

    #[test]
    fn test_malware_pattern() {
        let pattern = MalwarePattern::new("test_001", "Test Pattern", ThreatCategory::Cryptominer)
            .with_description("A test pattern")
            .with_threshold(0.75)
            .with_indicator(BehaviorIndicator::new("ind1", "type1").with_weight(0.5))
            .with_tags(vec!["test".to_string()]);

        assert_eq!(pattern.id, "test_001");
        assert_eq!(pattern.threshold, 0.75);
        assert_eq!(pattern.indicators.len(), 1);
        assert!(pattern.tags.contains(&"test".to_string()));
    }

    #[test]
    fn test_pattern_matcher_builtin() {
        let matcher = PatternMatcher::new();

        // Should have built-in patterns
        assert!(!matcher.is_empty());

        // Should have cryptominer patterns
        let crypto_patterns = matcher.patterns_by_category(ThreatCategory::Cryptominer);
        assert!(!crypto_patterns.is_empty());

        // Should have stealer patterns
        let stealer_patterns = matcher.patterns_by_category(ThreatCategory::Stealer);
        assert!(!stealer_patterns.is_empty());
    }

    #[test]
    fn test_pattern_matcher_add_remove() {
        let mut matcher = PatternMatcher::empty();
        assert!(matcher.is_empty());

        let pattern = MalwarePattern::new("custom_001", "Custom", ThreatCategory::Generic);
        matcher.add_pattern(pattern);

        assert_eq!(matcher.len(), 1);

        let removed = matcher.remove_pattern("custom_001");
        assert!(removed.is_some());
        assert!(matcher.is_empty());
    }

    #[test]
    fn test_pattern_matcher_tags() {
        let matcher = PatternMatcher::new();

        let crypto_tagged = matcher.patterns_by_tag("crypto");
        assert!(!crypto_tagged.is_empty());

        // All crypto-tagged patterns should be cryptominers
        for pattern in crypto_tagged {
            assert_eq!(pattern.category, ThreatCategory::Cryptominer);
        }
    }

    #[test]
    fn test_pattern_enable_disable() {
        let mut matcher = PatternMatcher::new();

        let pattern_id = &matcher.patterns()[0].id.clone();

        // Disable pattern
        assert!(matcher.set_pattern_enabled(pattern_id, false));

        // Verify disabled
        let pattern = matcher.patterns().iter().find(|p| &p.id == pattern_id).unwrap();
        assert!(!pattern.enabled);

        // Re-enable
        assert!(matcher.set_pattern_enabled(pattern_id, true));
    }

    #[test]
    fn test_enabled_patterns_iterator() {
        let mut matcher = PatternMatcher::new();
        let total = matcher.len();

        // Disable first pattern
        let first_id = matcher.patterns()[0].id.clone();
        matcher.set_pattern_enabled(&first_id, false);

        // Enabled count should be one less
        let enabled_count = matcher.enabled_patterns().count();
        assert_eq!(enabled_count, total - 1);
    }
}
