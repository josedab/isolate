//! eBPF-style syscall tracing for sandbox observability.
//!
//! Provides a framework for capturing and analyzing syscall-level events
//! from sandbox execution, enabling deep security analysis and anomaly detection.
//!
//! Note: Actual eBPF kernel integration requires Linux 5.10+. This module
//! provides the event model, correlation engine, and analysis pipeline that
//! can be fed from either real eBPF probes or simulated instrumentation.



use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// A captured syscall event from sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallEvent {
    /// Event ID (monotonically increasing).
    pub id: u64,
    /// Sandbox ID this event belongs to.
    pub sandbox_id: String,
    /// Syscall number.
    pub syscall_nr: u32,
    /// Syscall name.
    pub syscall_name: String,
    /// Arguments (up to 6).
    pub args: Vec<SyscallArg>,
    /// Return value.
    pub return_value: Option<i64>,
    /// Duration of the syscall.
    pub duration: Duration,
    /// Timestamp.
    pub timestamp: SystemTime,
    /// CPU core the syscall ran on.
    pub cpu: Option<u32>,
    /// Process/thread ID.
    pub pid: u32,
    /// Category for grouping.
    pub category: SyscallCategory,
}

/// Syscall argument representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyscallArg {
    Integer(i64),
    Pointer(u64),
    String(String),
    Buffer { addr: u64, len: usize },
    Fd(i32),
    Flags(u32),
}

/// Categories of syscalls for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyscallCategory {
    FileRead,
    FileWrite,
    FileOpen,
    FileClose,
    FileStat,
    NetworkConnect,
    NetworkSend,
    NetworkRecv,
    NetworkDns,
    ProcessControl,
    MemoryAlloc,
    MemoryMap,
    TimeRelated,
    SignalHandling,
    Other,
}

impl SyscallCategory {
    /// Check if this is a network-related syscall.
    pub fn is_network(&self) -> bool {
        matches!(
            self,
            Self::NetworkConnect | Self::NetworkSend | Self::NetworkRecv | Self::NetworkDns
        )
    }

    /// Check if this is file-related.
    pub fn is_file(&self) -> bool {
        matches!(
            self,
            Self::FileRead | Self::FileWrite | Self::FileOpen | Self::FileClose | Self::FileStat
        )
    }
}

/// Correlates syscall events with sandbox execution context.
pub struct EventCorrelator {
    /// Events indexed by sandbox.
    sandbox_events: HashMap<String, Vec<SyscallEvent>>,
    /// Next event ID.
    next_id: u64,
    /// Maximum events per sandbox.
    max_events_per_sandbox: usize,
    /// Total events processed.
    total_events: u64,
}

impl EventCorrelator {
    /// Create a new event correlator.
    pub fn new(max_events_per_sandbox: usize) -> Self {
        Self {
            sandbox_events: HashMap::new(),
            next_id: 0,
            max_events_per_sandbox,
            total_events: 0,
        }
    }

    /// Record a syscall event.
    pub fn record(&mut self, mut event: SyscallEvent) {
        event.id = self.next_id;
        self.next_id += 1;
        self.total_events += 1;

        let events = self.sandbox_events.entry(event.sandbox_id.clone()).or_default();

        // Evict oldest if at capacity
        if events.len() >= self.max_events_per_sandbox {
            events.remove(0);
        }

        events.push(event);
    }

    /// Get events for a sandbox.
    pub fn events_for(&self, sandbox_id: &str) -> &[SyscallEvent] {
        self.sandbox_events.get(sandbox_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get events by category for a sandbox.
    pub fn events_by_category(
        &self,
        sandbox_id: &str,
        category: SyscallCategory,
    ) -> Vec<&SyscallEvent> {
        self.events_for(sandbox_id)
            .iter()
            .filter(|e| e.category == category)
            .collect()
    }

    /// Generate a syscall profile for a sandbox.
    pub fn profile(&self, sandbox_id: &str) -> SyscallProfile {
        let events = self.events_for(sandbox_id);
        let mut category_counts: HashMap<SyscallCategory, u64> = HashMap::new();
        let mut syscall_counts: HashMap<String, u64> = HashMap::new();
        let mut total_duration = Duration::ZERO;

        for event in events {
            *category_counts.entry(event.category).or_default() += 1;
            *syscall_counts.entry(event.syscall_name.clone()).or_default() += 1;
            total_duration += event.duration;
        }

        SyscallProfile {
            sandbox_id: sandbox_id.to_string(),
            total_events: events.len() as u64,
            category_counts,
            syscall_counts,
            total_syscall_time: total_duration,
            unique_files_accessed: self.count_unique_files(events),
            network_connections: self.count_network_connections(events),
        }
    }

    fn count_unique_files(&self, events: &[SyscallEvent]) -> usize {
        let mut files = std::collections::HashSet::new();
        for event in events {
            if event.category.is_file() {
                for arg in &event.args {
                    if let SyscallArg::String(path) = arg {
                        files.insert(path.clone());
                    }
                }
            }
        }
        files.len()
    }

    fn count_network_connections(&self, events: &[SyscallEvent]) -> usize {
        events.iter().filter(|e| e.category == SyscallCategory::NetworkConnect).count()
    }

    /// Total events processed.
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// Clear events for a sandbox.
    pub fn clear(&mut self, sandbox_id: &str) {
        self.sandbox_events.remove(sandbox_id);
    }
}

impl Default for EventCorrelator {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// Syscall profile summary for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallProfile {
    pub sandbox_id: String,
    pub total_events: u64,
    pub category_counts: HashMap<SyscallCategory, u64>,
    pub syscall_counts: HashMap<String, u64>,
    pub total_syscall_time: Duration,
    pub unique_files_accessed: usize,
    pub network_connections: usize,
}

/// Security anomaly detected from syscall patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAnomaly {
    /// Anomaly type.
    pub anomaly_type: AnomalyType,
    /// Severity (1-10).
    pub severity: u8,
    /// Description.
    pub description: String,
    /// Related events.
    pub related_event_ids: Vec<u64>,
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Timestamp of detection.
    pub detected_at: SystemTime,
    /// Recommended action.
    pub recommended_action: RecommendedAction,
}

/// Types of security anomalies detectable from syscall patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Unusual number of network connections.
    ExcessiveNetworkActivity,
    /// Accessing unexpected files.
    SuspiciousFileAccess,
    /// Potential port scanning.
    PortScanning,
    /// DNS exfiltration pattern.
    DnsExfiltration,
    /// Path traversal attempt.
    PathTraversal,
    /// Cryptomining indicators.
    CryptominingPattern,
    /// Excessive memory allocation.
    MemoryAbuse,
    /// Unusual syscall sequence.
    AbnormalSyscallSequence,
}

/// Recommended action for anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendedAction {
    Monitor,
    Throttle,
    Alert,
    Suspend,
    Terminate,
}

/// Analyzes syscall patterns for security anomalies.
pub struct AnomalyAnalyzer {
    /// Detection rules.
    rules: Vec<DetectionRule>,
    /// Detected anomalies.
    anomalies: Vec<SecurityAnomaly>,
}

/// A rule for detecting anomalies.
#[derive(Debug, Clone)]
pub struct DetectionRule {
    pub name: String,
    pub anomaly_type: AnomalyType,
    pub severity: u8,
    pub threshold: f64,
    pub window: Duration,
    pub action: RecommendedAction,
}

impl AnomalyAnalyzer {
    /// Create with default detection rules.
    pub fn new() -> Self {
        Self { rules: Self::default_rules(), anomalies: Vec::new() }
    }

    fn default_rules() -> Vec<DetectionRule> {
        vec![
            DetectionRule {
                name: "excessive-network".to_string(),
                anomaly_type: AnomalyType::ExcessiveNetworkActivity,
                severity: 7,
                threshold: 100.0,
                window: Duration::from_secs(60),
                action: RecommendedAction::Throttle,
            },
            DetectionRule {
                name: "port-scanning".to_string(),
                anomaly_type: AnomalyType::PortScanning,
                severity: 9,
                threshold: 20.0,
                window: Duration::from_secs(10),
                action: RecommendedAction::Terminate,
            },
            DetectionRule {
                name: "path-traversal".to_string(),
                anomaly_type: AnomalyType::PathTraversal,
                severity: 8,
                threshold: 1.0,
                window: Duration::from_secs(60),
                action: RecommendedAction::Suspend,
            },
            DetectionRule {
                name: "cryptomining".to_string(),
                anomaly_type: AnomalyType::CryptominingPattern,
                severity: 10,
                threshold: 0.95,
                window: Duration::from_secs(30),
                action: RecommendedAction::Terminate,
            },
        ]
    }

    /// Analyze a syscall profile for anomalies.
    pub fn analyze(&mut self, profile: &SyscallProfile) -> Vec<SecurityAnomaly> {
        let mut detected = Vec::new();

        for rule in &self.rules {
            if let Some(anomaly) = self.check_rule(rule, profile) {
                detected.push(anomaly);
            }
        }

        self.anomalies.extend(detected.clone());
        detected
    }

    fn check_rule(
        &self,
        rule: &DetectionRule,
        profile: &SyscallProfile,
    ) -> Option<SecurityAnomaly> {
        let triggered = match rule.anomaly_type {
            AnomalyType::ExcessiveNetworkActivity => {
                profile.network_connections as f64 > rule.threshold
            }
            AnomalyType::PortScanning => {
                let connects =
                    profile.category_counts.get(&SyscallCategory::NetworkConnect).unwrap_or(&0);
                *connects as f64 > rule.threshold
            }
            AnomalyType::PathTraversal => {
                profile.unique_files_accessed as f64 > rule.threshold * 10.0
            }
            AnomalyType::CryptominingPattern => {
                // High CPU with minimal I/O is suspicious
                let total = profile.total_events.max(1) as f64;
                let io_events = profile
                    .category_counts
                    .iter()
                    .filter(|(cat, _)| cat.is_file() || cat.is_network())
                    .map(|(_, count)| *count as f64)
                    .sum::<f64>();
                let io_ratio = io_events / total;
                io_ratio < (1.0 - rule.threshold) && total > 1000.0
            }
            _ => false,
        };

        if triggered {
            Some(SecurityAnomaly {
                anomaly_type: rule.anomaly_type,
                severity: rule.severity,
                description: format!("Rule '{}' triggered", rule.name),
                related_event_ids: Vec::new(),
                sandbox_id: profile.sandbox_id.clone(),
                detected_at: SystemTime::now(),
                recommended_action: rule.action,
            })
        } else {
            None
        }
    }

    /// Get all detected anomalies.
    pub fn anomalies(&self) -> &[SecurityAnomaly] {
        &self.anomalies
    }

    /// Add a custom detection rule.
    pub fn add_rule(&mut self, rule: DetectionRule) {
        self.rules.push(rule);
    }

    /// Clear detected anomalies.
    pub fn clear(&mut self) {
        self.anomalies.clear();
    }
}

impl Default for AnomalyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(sandbox_id: &str, name: &str, category: SyscallCategory) -> SyscallEvent {
        SyscallEvent {
            id: 0,
            sandbox_id: sandbox_id.to_string(),
            syscall_nr: 0,
            syscall_name: name.to_string(),
            args: Vec::new(),
            return_value: Some(0),
            duration: Duration::from_micros(10),
            timestamp: SystemTime::now(),
            cpu: Some(0),
            pid: 1000,
            category,
        }
    }

    #[test]
    fn test_event_correlator() {
        let mut correlator = EventCorrelator::new(100);

        correlator.record(make_event("sb-1", "open", SyscallCategory::FileOpen));
        correlator.record(make_event("sb-1", "read", SyscallCategory::FileRead));
        correlator.record(make_event("sb-2", "connect", SyscallCategory::NetworkConnect));

        assert_eq!(correlator.events_for("sb-1").len(), 2);
        assert_eq!(correlator.events_for("sb-2").len(), 1);
        assert_eq!(correlator.total_events(), 3);
    }

    #[test]
    fn test_event_correlator_eviction() {
        let mut correlator = EventCorrelator::new(2);

        correlator.record(make_event("sb-1", "a", SyscallCategory::Other));
        correlator.record(make_event("sb-1", "b", SyscallCategory::Other));
        correlator.record(make_event("sb-1", "c", SyscallCategory::Other));

        assert_eq!(correlator.events_for("sb-1").len(), 2);
        assert_eq!(correlator.events_for("sb-1")[0].syscall_name, "b");
    }

    #[test]
    fn test_syscall_profile() {
        let mut correlator = EventCorrelator::new(100);

        correlator.record(make_event("sb-1", "open", SyscallCategory::FileOpen));
        correlator.record(make_event("sb-1", "read", SyscallCategory::FileRead));
        correlator.record(make_event("sb-1", "connect", SyscallCategory::NetworkConnect));

        let profile = correlator.profile("sb-1");
        assert_eq!(profile.total_events, 3);
        assert_eq!(profile.network_connections, 1);
    }

    #[test]
    fn test_events_by_category() {
        let mut correlator = EventCorrelator::new(100);

        correlator.record(make_event("sb-1", "read", SyscallCategory::FileRead));
        correlator.record(make_event("sb-1", "write", SyscallCategory::FileWrite));
        correlator.record(make_event("sb-1", "connect", SyscallCategory::NetworkConnect));

        let file_events = correlator.events_by_category("sb-1", SyscallCategory::FileRead);
        assert_eq!(file_events.len(), 1);
    }

    #[test]
    fn test_anomaly_analyzer_excessive_network() {
        let mut analyzer = AnomalyAnalyzer::new();

        let profile = SyscallProfile {
            sandbox_id: "sb-1".to_string(),
            total_events: 200,
            category_counts: {
                let mut m = HashMap::new();
                m.insert(SyscallCategory::NetworkConnect, 150);
                m
            },
            syscall_counts: HashMap::new(),
            total_syscall_time: Duration::from_secs(10),
            unique_files_accessed: 0,
            network_connections: 150,
        };

        let anomalies = analyzer.analyze(&profile);
        assert!(!anomalies.is_empty());
        assert!(anomalies
            .iter()
            .any(|a| a.anomaly_type == AnomalyType::ExcessiveNetworkActivity));
    }

    #[test]
    fn test_anomaly_analyzer_clean_profile() {
        let mut analyzer = AnomalyAnalyzer::new();

        let profile = SyscallProfile {
            sandbox_id: "sb-1".to_string(),
            total_events: 50,
            category_counts: {
                let mut m = HashMap::new();
                m.insert(SyscallCategory::FileRead, 30);
                m.insert(SyscallCategory::FileWrite, 20);
                m
            },
            syscall_counts: HashMap::new(),
            total_syscall_time: Duration::from_secs(1),
            unique_files_accessed: 5,
            network_connections: 2,
        };

        let anomalies = analyzer.analyze(&profile);
        assert!(anomalies.is_empty());
    }

    #[test]
    fn test_syscall_category_classification() {
        assert!(SyscallCategory::NetworkConnect.is_network());
        assert!(!SyscallCategory::FileRead.is_network());
        assert!(SyscallCategory::FileWrite.is_file());
        assert!(!SyscallCategory::NetworkSend.is_file());
    }

    #[test]
    fn test_custom_detection_rule() {
        let mut analyzer = AnomalyAnalyzer::new();
        analyzer.add_rule(DetectionRule {
            name: "custom".to_string(),
            anomaly_type: AnomalyType::ExcessiveNetworkActivity,
            severity: 5,
            threshold: 5.0,
            window: Duration::from_secs(30),
            action: RecommendedAction::Alert,
        });

        assert_eq!(analyzer.rules.len(), 5); // 4 default + 1 custom
    }
}
