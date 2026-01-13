//! Network policy engine with declarative rule evaluation.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Action to take when a policy rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    /// Allow the network operation.
    Allow,
    /// Deny the network operation.
    Deny,
}

/// A single policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Rule name (for logging/debugging).
    pub name: String,
    /// Action to take when this rule matches.
    pub action: PolicyAction,
    /// Match condition.
    pub condition: PolicyCondition,
    /// Priority (higher = evaluated first).
    pub priority: i32,
}

/// Condition for matching a network operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyCondition {
    /// Match HTTP requests to a host pattern (supports wildcards).
    HttpHost { pattern: String },
    /// Match TCP connections to a CIDR range.
    Cidr { cidr: String, prefix_len: u8 },
    /// Match TCP connections to a specific port.
    Port { port: u16 },
    /// Match TCP connections to a specific port range.
    PortRange { start: u16, end: u16 },
    /// Match DNS resolution requests for a domain pattern.
    DnsDomain { pattern: String },
    /// Match any network operation (catch-all).
    Any,
}

impl PolicyCondition {
    /// Check if a hostname matches this condition.
    pub fn matches_host(&self, host: &str) -> bool {
        match self {
            PolicyCondition::HttpHost { pattern } => host_matches_pattern(host, pattern),
            PolicyCondition::DnsDomain { pattern } => host_matches_pattern(host, pattern),
            PolicyCondition::Any => true,
            _ => false,
        }
    }

    /// Check if a port matches this condition.
    pub fn matches_port(&self, port: u16) -> bool {
        match self {
            PolicyCondition::Port { port: p } => *p == port,
            PolicyCondition::PortRange { start, end } => port >= *start && port <= *end,
            PolicyCondition::Any => true,
            _ => false,
        }
    }

    /// Check if an IP address matches this condition.
    pub fn matches_ip(&self, ip: &IpAddr) -> bool {
        match self {
            PolicyCondition::Cidr { cidr, prefix_len } => ip_matches_cidr(ip, cidr, *prefix_len),
            PolicyCondition::Any => true,
            _ => false,
        }
    }
}

/// Network policy configuration.
#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    /// Ordered list of rules (evaluated by priority, highest first).
    rules: Vec<PolicyRule>,
    /// Default action when no rule matches.
    default_action: PolicyAction,
    /// Maximum concurrent connections.
    max_connections: usize,
    /// Rate limit: max requests per window.
    rate_limit: Option<RateLimitConfig>,
    /// Whether TLS is required for all connections.
    require_tls: bool,
    /// Maximum request body size.
    max_request_body: usize,
    /// Maximum response body size.
    max_response_body: usize,
    /// Connection timeout.
    connection_timeout: Duration,
}

/// Rate limiting configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of requests in the window.
    pub max_requests: u64,
    /// Window duration.
    pub window: Duration,
}

impl NetworkPolicy {
    /// Create a policy builder.
    pub fn builder() -> NetworkPolicyBuilder {
        NetworkPolicyBuilder::default()
    }

    /// Check if an HTTP request to the given host is allowed.
    pub fn allows_http_host(&self, host: &str) -> bool {
        self.evaluate_host(host) == PolicyAction::Allow
    }

    /// Check if a TCP connection to the given host:port is allowed.
    pub fn allows_tcp(&self, host: &str, port: u16) -> bool {
        let host_allowed = self.evaluate_host(host) == PolicyAction::Allow;
        let port_allowed = self.evaluate_port(port) == PolicyAction::Allow;
        host_allowed && port_allowed
    }

    /// Check if DNS resolution for the given domain is allowed.
    pub fn allows_dns(&self, domain: &str) -> bool {
        self.evaluate_host(domain) == PolicyAction::Allow
    }

    /// Check if an IP address is allowed.
    pub fn allows_ip(&self, ip: &IpAddr) -> bool {
        self.evaluate_ip(ip) == PolicyAction::Allow
    }

    /// Get whether TLS is required.
    pub fn requires_tls(&self) -> bool {
        self.require_tls
    }

    /// Get max concurrent connections.
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// Get the connection timeout.
    pub fn connection_timeout(&self) -> Duration {
        self.connection_timeout
    }

    /// Get the rate limit config.
    pub fn rate_limit(&self) -> Option<&RateLimitConfig> {
        self.rate_limit.as_ref()
    }

    /// Get all rules.
    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    /// Evaluate a host against the rules.
    fn evaluate_host(&self, host: &str) -> PolicyAction {
        // Rules are sorted by priority (highest first)
        for rule in &self.rules {
            if rule.condition.matches_host(host) {
                return rule.action;
            }
        }
        self.default_action
    }

    /// Evaluate a port against the rules.
    fn evaluate_port(&self, port: u16) -> PolicyAction {
        for rule in &self.rules {
            if rule.condition.matches_port(port) {
                return rule.action;
            }
        }
        self.default_action
    }

    /// Evaluate an IP address against the rules.
    fn evaluate_ip(&self, ip: &IpAddr) -> PolicyAction {
        for rule in &self.rules {
            if rule.condition.matches_ip(ip) {
                return rule.action;
            }
        }
        self.default_action
    }
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_action: PolicyAction::Deny,
            max_connections: 10,
            rate_limit: None,
            require_tls: true,
            max_request_body: 10 * 1024 * 1024,
            max_response_body: 100 * 1024 * 1024,
            connection_timeout: Duration::from_secs(30),
        }
    }
}

/// Builder for network policy.
#[derive(Debug, Default)]
pub struct NetworkPolicyBuilder {
    rules: Vec<PolicyRule>,
    default_action: Option<PolicyAction>,
    max_connections: Option<usize>,
    rate_limit: Option<RateLimitConfig>,
    require_tls: Option<bool>,
    max_request_body: Option<usize>,
    max_response_body: Option<usize>,
    connection_timeout: Option<Duration>,
    next_priority: i32,
}

impl NetworkPolicyBuilder {
    /// Allow HTTP access to a host pattern (supports `*` wildcards).
    pub fn allow_http(mut self, pattern: impl Into<String>) -> Self {
        self.next_priority += 1;
        self.rules.push(PolicyRule {
            name: format!("allow_http_{}", self.next_priority),
            action: PolicyAction::Allow,
            condition: PolicyCondition::HttpHost { pattern: pattern.into() },
            priority: self.next_priority,
        });
        self
    }

    /// Deny HTTP access to a host pattern.
    pub fn deny_http(mut self, pattern: impl Into<String>) -> Self {
        self.next_priority += 1;
        self.rules.push(PolicyRule {
            name: format!("deny_http_{}", self.next_priority),
            action: PolicyAction::Deny,
            condition: PolicyCondition::HttpHost { pattern: pattern.into() },
            priority: self.next_priority,
        });
        self
    }

    /// Deny access to a CIDR range (e.g., "10.0.0.0/8" for private networks).
    pub fn deny_cidr(mut self, cidr: impl Into<String>) -> Self {
        let cidr_str = cidr.into();
        let (addr, prefix) = parse_cidr(&cidr_str);
        self.next_priority += 1;
        self.rules.push(PolicyRule {
            name: format!("deny_cidr_{}", self.next_priority),
            action: PolicyAction::Deny,
            condition: PolicyCondition::Cidr { cidr: addr, prefix_len: prefix },
            priority: self.next_priority,
        });
        self
    }

    /// Allow DNS resolution for a domain pattern.
    pub fn allow_dns(mut self, pattern: impl Into<String>) -> Self {
        self.next_priority += 1;
        self.rules.push(PolicyRule {
            name: format!("allow_dns_{}", self.next_priority),
            action: PolicyAction::Allow,
            condition: PolicyCondition::DnsDomain { pattern: pattern.into() },
            priority: self.next_priority,
        });
        self
    }

    /// Allow TCP access to a specific port.
    pub fn allow_port(mut self, port: u16) -> Self {
        self.next_priority += 1;
        self.rules.push(PolicyRule {
            name: format!("allow_port_{}", port),
            action: PolicyAction::Allow,
            condition: PolicyCondition::Port { port },
            priority: self.next_priority,
        });
        self
    }

    /// Add a custom rule.
    pub fn rule(mut self, rule: PolicyRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set the default action (when no rule matches).
    pub fn default_action(mut self, action: PolicyAction) -> Self {
        self.default_action = Some(action);
        self
    }

    /// Set maximum concurrent connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = Some(max);
        self
    }

    /// Set rate limiting.
    pub fn rate_limit(mut self, max_requests: u64, window: Duration) -> Self {
        self.rate_limit = Some(RateLimitConfig { max_requests, window });
        self
    }

    /// Set whether TLS is required.
    pub fn require_tls(mut self, require: bool) -> Self {
        self.require_tls = Some(require);
        self
    }

    /// Set connection timeout.
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = Some(timeout);
        self
    }

    /// Build the policy.
    pub fn build(mut self) -> NetworkPolicy {
        // Sort rules by priority (highest first)
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        let defaults = NetworkPolicy::default();
        NetworkPolicy {
            rules: self.rules,
            default_action: self.default_action.unwrap_or(defaults.default_action),
            max_connections: self.max_connections.unwrap_or(defaults.max_connections),
            rate_limit: self.rate_limit,
            require_tls: self.require_tls.unwrap_or(defaults.require_tls),
            max_request_body: self.max_request_body.unwrap_or(defaults.max_request_body),
            max_response_body: self.max_response_body.unwrap_or(defaults.max_response_body),
            connection_timeout: self.connection_timeout.unwrap_or(defaults.connection_timeout),
        }
    }
}

/// Runtime rate limiter using a sliding window.
pub struct RateLimiter {
    config: RateLimitConfig,
    requests: Mutex<Vec<Instant>>,
    total_allowed: AtomicU64,
    total_denied: AtomicU64,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            requests: Mutex::new(Vec::new()),
            total_allowed: AtomicU64::new(0),
            total_denied: AtomicU64::new(0),
        }
    }

    /// Check if a request is allowed. Returns true and records it if allowed.
    pub fn check(&self) -> bool {
        let now = Instant::now();
        let mut requests = self.requests.lock();

        // Remove expired entries
        let window_start = now - self.config.window;
        requests.retain(|&t| t >= window_start);

        if requests.len() as u64 >= self.config.max_requests {
            self.total_denied.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            requests.push(now);
            self.total_allowed.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    /// Get the number of requests in the current window.
    pub fn current_count(&self) -> u64 {
        let now = Instant::now();
        let requests = self.requests.lock();
        let window_start = now - self.config.window;
        requests.iter().filter(|&&t| t >= window_start).count() as u64
    }

    /// Get total allowed requests.
    pub fn total_allowed(&self) -> u64 {
        self.total_allowed.load(Ordering::Relaxed)
    }

    /// Get total denied requests.
    pub fn total_denied(&self) -> u64 {
        self.total_denied.load(Ordering::Relaxed)
    }
}

/// Connection counter for enforcing max connections.
pub struct ConnectionCounter {
    max: usize,
    current: AtomicU64,
}

impl ConnectionCounter {
    /// Create a new counter with a maximum.
    pub fn new(max: usize) -> Self {
        Self { max, current: AtomicU64::new(0) }
    }

    /// Try to acquire a connection slot. Returns true if successful.
    pub fn acquire(&self) -> bool {
        let current = self.current.load(Ordering::Relaxed);
        if current as usize >= self.max {
            return false;
        }
        // Use CAS to avoid race conditions
        self.current
            .compare_exchange(current, current + 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Release a connection slot.
    pub fn release(&self) {
        self.current.fetch_sub(1, Ordering::Release);
    }

    /// Get current connection count.
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }
}

/// Policy evaluation context for audit logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// The operation that was evaluated.
    pub operation: String,
    /// The target (host, IP, etc.).
    pub target: String,
    /// The action taken.
    pub action: PolicyAction,
    /// Which rule matched (if any).
    pub matched_rule: Option<String>,
    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Audit log for network policy decisions.
pub struct PolicyAuditLog {
    decisions: Arc<Mutex<Vec<PolicyDecision>>>,
    max_entries: usize,
}

impl PolicyAuditLog {
    /// Create a new audit log.
    pub fn new(max_entries: usize) -> Self {
        Self { decisions: Arc::new(Mutex::new(Vec::new())), max_entries }
    }

    /// Record a policy decision.
    pub fn record(&self, decision: PolicyDecision) {
        let mut decisions = self.decisions.lock();
        if decisions.len() >= self.max_entries {
            decisions.remove(0);
        }
        decisions.push(decision);
    }

    /// Get all recorded decisions.
    pub fn decisions(&self) -> Vec<PolicyDecision> {
        self.decisions.lock().clone()
    }

    /// Get the number of deny decisions.
    pub fn deny_count(&self) -> usize {
        self.decisions.lock().iter().filter(|d| d.action == PolicyAction::Deny).count()
    }

    /// Clear the audit log.
    pub fn clear(&self) {
        self.decisions.lock().clear();
    }
}

impl Default for PolicyAuditLog {
    fn default() -> Self {
        Self::new(1000)
    }
}

// Helper functions

fn host_matches_pattern(host: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with("*.") {
        let suffix = &pattern[1..]; // ".example.com"
        host.ends_with(suffix) || host == &pattern[2..]
    } else {
        host == pattern
    }
}

fn parse_cidr(cidr: &str) -> (String, u8) {
    if let Some(pos) = cidr.find('/') {
        let addr = cidr[..pos].to_string();
        let prefix = cidr[pos + 1..].parse::<u8>().unwrap_or(32);
        (addr, prefix)
    } else {
        (cidr.to_string(), 32)
    }
}

fn ip_matches_cidr(ip: &IpAddr, cidr_addr: &str, prefix_len: u8) -> bool {
    let cidr_ip: IpAddr = match cidr_addr.parse() {
        Ok(ip) => ip,
        Err(_) => return false,
    };

    match (ip, cidr_ip) {
        (IpAddr::V4(ip), IpAddr::V4(cidr)) => {
            let ip_bits = u32::from(*ip);
            let cidr_bits = u32::from(cidr);
            let mask = if prefix_len >= 32 { u32::MAX } else { u32::MAX << (32 - prefix_len) };
            (ip_bits & mask) == (cidr_bits & mask)
        }
        (IpAddr::V6(ip), IpAddr::V6(cidr)) => {
            let ip_bits = u128::from(*ip);
            let cidr_bits = u128::from(cidr);
            let mask = if prefix_len >= 128 { u128::MAX } else { u128::MAX << (128 - prefix_len) };
            (ip_bits & mask) == (cidr_bits & mask)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_matching() {
        assert!(host_matches_pattern("api.example.com", "api.example.com"));
        assert!(!host_matches_pattern("other.com", "api.example.com"));

        assert!(host_matches_pattern("sub.example.com", "*.example.com"));
        assert!(host_matches_pattern("example.com", "*.example.com"));
        assert!(!host_matches_pattern("other.com", "*.example.com"));

        assert!(host_matches_pattern("anything", "*"));
    }

    #[test]
    fn test_cidr_matching() {
        let ip: IpAddr = "10.0.1.5".parse().unwrap();
        assert!(ip_matches_cidr(&ip, "10.0.0.0", 8));
        assert!(ip_matches_cidr(&ip, "10.0.1.0", 24));
        assert!(!ip_matches_cidr(&ip, "192.168.0.0", 16));
    }

    #[test]
    fn test_policy_builder() {
        let policy = NetworkPolicy::builder()
            .allow_http("*.api.example.com")
            .allow_http("cdn.trusted.com")
            .deny_cidr("10.0.0.0/8")
            .max_connections(10)
            .rate_limit(100, Duration::from_secs(60))
            .require_tls(true)
            .build();

        assert!(policy.allows_http_host("sub.api.example.com"));
        assert!(policy.allows_http_host("cdn.trusted.com"));
        assert!(!policy.allows_http_host("evil.com"));
        assert!(policy.requires_tls());
        assert_eq!(policy.max_connections(), 10);
    }

    #[test]
    fn test_default_deny() {
        let policy = NetworkPolicy::default();
        assert!(!policy.allows_http_host("any.host.com"));
    }

    #[test]
    fn test_rate_limiter() {
        let limiter =
            RateLimiter::new(RateLimitConfig { max_requests: 3, window: Duration::from_secs(60) });

        assert!(limiter.check());
        assert!(limiter.check());
        assert!(limiter.check());
        assert!(!limiter.check()); // Rate limited

        assert_eq!(limiter.total_allowed(), 3);
        assert_eq!(limiter.total_denied(), 1);
    }

    #[test]
    fn test_connection_counter() {
        let counter = ConnectionCounter::new(2);

        assert!(counter.acquire());
        assert!(counter.acquire());
        assert!(!counter.acquire()); // At limit

        counter.release();
        assert!(counter.acquire()); // Slot freed
    }

    #[test]
    fn test_policy_audit_log() {
        let log = PolicyAuditLog::new(3);

        log.record(PolicyDecision {
            operation: "http".to_string(),
            target: "example.com".to_string(),
            action: PolicyAction::Allow,
            matched_rule: Some("allow_http_1".to_string()),
            timestamp: chrono::Utc::now(),
        });

        log.record(PolicyDecision {
            operation: "http".to_string(),
            target: "evil.com".to_string(),
            action: PolicyAction::Deny,
            matched_rule: None,
            timestamp: chrono::Utc::now(),
        });

        assert_eq!(log.decisions().len(), 2);
        assert_eq!(log.deny_count(), 1);
    }

    #[test]
    fn test_policy_with_ports() {
        let policy =
            NetworkPolicy::builder().allow_http("*").allow_port(443).allow_port(80).build();

        assert!(policy.allows_tcp("example.com", 443));
        assert!(policy.allows_tcp("example.com", 80));
    }
}
