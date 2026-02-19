//! Natural Language Policy Definition
//!
//! Define security policies using natural language:
//! - "Allow network access only to api.example.com"
//! - "Block file writes except to /tmp"
//! - AI-powered policy interpretation and enforcement

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.


use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Natural language policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NaturalPolicy {
    /// Policy ID.
    pub id: String,
    /// Natural language description.
    pub description: String,
    /// Parsed policy rules.
    pub rules: Vec<PolicyRule>,
    /// Policy priority (higher = more important).
    pub priority: u32,
    /// Is policy enabled.
    pub enabled: bool,
}

/// Parsed policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Rule action.
    pub action: PolicyAction,
    /// Resource type.
    pub resource: ResourceKind,
    /// Conditions.
    pub conditions: Vec<Condition>,
    /// Original text that generated this rule.
    pub source_text: String,
}

/// Policy action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    Allow,
    Deny,
    Audit,
    RateLimit,
}

/// Resource kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    /// Network access.
    Network,
    /// File system.
    FileSystem,
    /// Process execution.
    Process,
    /// Environment variables.
    Environment,
    /// Memory operations.
    Memory,
    /// System calls.
    Syscall,
    /// All resources.
    All,
}

/// Condition for policy matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    /// Match host pattern.
    Host(String),
    /// Match path pattern.
    Path(String),
    /// Match port.
    Port(u16),
    /// Match protocol.
    Protocol(String),
    /// Match operation type.
    Operation(String),
    /// Match time range.
    TimeRange { start: String, end: String },
    /// Rate limit.
    RateLimit { max: u32, period_secs: u32 },
    /// Custom condition.
    Custom { key: String, value: String },
}

/// Policy evaluation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Final decision.
    pub action: PolicyAction,
    /// Matching policies.
    pub matching_policies: Vec<String>,
    /// Explanation.
    pub explanation: String,
    /// Confidence score (0.0-1.0).
    pub confidence: f64,
}

/// Policy parser configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserConfig {
    /// Enable fuzzy matching.
    pub fuzzy_matching: bool,
    /// Confidence threshold.
    pub confidence_threshold: f64,
    /// Default action when no policy matches.
    pub default_action: PolicyAction,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self { fuzzy_matching: true, confidence_threshold: 0.7, default_action: PolicyAction::Deny }
    }
}

/// Natural language policy parser.
pub struct PolicyParser {
    config: ParserConfig,
    keywords: HashMap<String, KeywordInfo>,
}

/// Information about a keyword.
#[derive(Debug, Clone)]
struct KeywordInfo {
    action: Option<PolicyAction>,
    resource: Option<ResourceKind>,
    weight: f64,
}

impl Default for PolicyParser {
    fn default() -> Self {
        Self::new(ParserConfig::default())
    }
}

impl PolicyParser {
    /// Create a new parser.
    pub fn new(config: ParserConfig) -> Self {
        let mut keywords = HashMap::new();

        // Action keywords
        for (word, action) in [
            ("allow", PolicyAction::Allow),
            ("permit", PolicyAction::Allow),
            ("enable", PolicyAction::Allow),
            ("grant", PolicyAction::Allow),
            ("deny", PolicyAction::Deny),
            ("block", PolicyAction::Deny),
            ("forbid", PolicyAction::Deny),
            ("prevent", PolicyAction::Deny),
            ("reject", PolicyAction::Deny),
            ("audit", PolicyAction::Audit),
            ("log", PolicyAction::Audit),
            ("monitor", PolicyAction::Audit),
            ("limit", PolicyAction::RateLimit),
            ("throttle", PolicyAction::RateLimit),
        ] {
            keywords.insert(
                word.to_string(),
                KeywordInfo { action: Some(action), resource: None, weight: 1.0 },
            );
        }

        // Resource keywords
        for (word, resource) in [
            ("network", ResourceKind::Network),
            ("internet", ResourceKind::Network),
            ("http", ResourceKind::Network),
            ("https", ResourceKind::Network),
            ("connection", ResourceKind::Network),
            ("file", ResourceKind::FileSystem),
            ("filesystem", ResourceKind::FileSystem),
            ("directory", ResourceKind::FileSystem),
            ("folder", ResourceKind::FileSystem),
            ("read", ResourceKind::FileSystem),
            ("write", ResourceKind::FileSystem),
            ("process", ResourceKind::Process),
            ("execute", ResourceKind::Process),
            ("spawn", ResourceKind::Process),
            ("memory", ResourceKind::Memory),
            ("environment", ResourceKind::Environment),
            ("env", ResourceKind::Environment),
            ("syscall", ResourceKind::Syscall),
        ] {
            keywords.insert(
                word.to_string(),
                KeywordInfo { action: None, resource: Some(resource), weight: 0.8 },
            );
        }

        Self { config, keywords }
    }

    /// Parse natural language into policy.
    pub fn parse(&self, text: &str) -> Result<NaturalPolicy, ParseError> {
        let text = text.to_lowercase();
        let words: Vec<&str> = text.split_whitespace().collect();

        if words.is_empty() {
            return Err(ParseError::EmptyInput);
        }

        // Extract action
        let action = self.extract_action(&words)?;

        // Extract resource type
        let resource = self.extract_resource(&words);

        // Extract conditions
        let conditions = self.extract_conditions(&text);

        let rule = PolicyRule { action, resource, conditions, source_text: text.clone() };

        Ok(NaturalPolicy {
            id: generate_id(),
            description: text,
            rules: vec![rule],
            priority: 100,
            enabled: true,
        })
    }

    fn extract_action(&self, words: &[&str]) -> Result<PolicyAction, ParseError> {
        for word in words {
            if let Some(info) = self.keywords.get(*word) {
                if let Some(action) = info.action {
                    return Ok(action);
                }
            }
        }

        // Check for negation patterns
        if words.contains(&"no") || words.contains(&"not") || words.contains(&"don't") {
            return Ok(PolicyAction::Deny);
        }

        if words.contains(&"only") || words.contains(&"except") {
            return Ok(PolicyAction::Allow);
        }

        Err(ParseError::NoActionFound)
    }

    fn extract_resource(&self, words: &[&str]) -> ResourceKind {
        for word in words {
            if let Some(info) = self.keywords.get(*word) {
                if let Some(resource) = &info.resource {
                    return resource.clone();
                }
            }
        }

        ResourceKind::All
    }

    fn extract_conditions(&self, text: &str) -> Vec<Condition> {
        let mut conditions = Vec::new();

        // Extract hosts/domains
        for word in text.split_whitespace() {
            if word.contains('.') && !word.starts_with('/') {
                // Looks like a domain
                conditions.push(Condition::Host(word.to_string()));
            }

            if word.starts_with('/') {
                // Looks like a path
                conditions.push(Condition::Path(word.to_string()));
            }
        }

        // Extract ports
        if let Some(idx) = text.find("port") {
            let after = &text[idx..];
            for word in after.split_whitespace() {
                if let Ok(port) = word.parse::<u16>() {
                    conditions.push(Condition::Port(port));
                    break;
                }
            }
        }

        // Extract rate limits
        if text.contains("per second") || text.contains("/s") {
            if let Some(num) = extract_number(text) {
                conditions.push(Condition::RateLimit { max: num as u32, period_secs: 1 });
            }
        }

        conditions
    }
}

/// Policy engine for evaluation.
pub struct PolicyEngine {
    policies: Vec<NaturalPolicy>,
    parser: PolicyParser,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    /// Create a new policy engine.
    pub fn new() -> Self {
        Self { policies: Vec::new(), parser: PolicyParser::default() }
    }

    /// Add a natural language policy.
    pub fn add_policy(&mut self, text: &str) -> Result<String, ParseError> {
        let policy = self.parser.parse(text)?;
        let id = policy.id.clone();
        self.policies.push(policy);
        Ok(id)
    }

    /// Add a pre-parsed policy.
    pub fn add_parsed_policy(&mut self, policy: NaturalPolicy) {
        self.policies.push(policy);
    }

    /// Remove a policy.
    pub fn remove_policy(&mut self, id: &str) -> bool {
        let len = self.policies.len();
        self.policies.retain(|p| p.id != id);
        self.policies.len() < len
    }

    /// Enable/disable a policy.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        for policy in &mut self.policies {
            if policy.id == id {
                policy.enabled = enabled;
                return true;
            }
        }
        false
    }

    /// Evaluate policies for a request.
    pub fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        let mut matching_policies = Vec::new();
        let mut action = self.parser.config.default_action;
        let mut highest_priority = 0;

        for policy in &self.policies {
            if !policy.enabled {
                continue;
            }

            for rule in &policy.rules {
                if self.matches_rule(rule, request) {
                    matching_policies.push(policy.id.clone());

                    if policy.priority > highest_priority {
                        highest_priority = policy.priority;
                        action = rule.action;
                    }
                }
            }
        }

        let confidence = if matching_policies.is_empty() { 0.5 } else { 0.9 };
        let explanation = if matching_policies.is_empty() {
            "No matching policies, using default action".to_string()
        } else {
            format!("Matched {} policies", matching_policies.len())
        };

        PolicyDecision { action, matching_policies, explanation, confidence }
    }

    fn matches_rule(&self, rule: &PolicyRule, request: &PolicyRequest) -> bool {
        // Check resource type
        if rule.resource != ResourceKind::All && rule.resource != request.resource {
            return false;
        }

        // Check conditions
        for condition in &rule.conditions {
            if !self.matches_condition(condition, request) {
                return false;
            }
        }

        true
    }

    fn matches_condition(&self, condition: &Condition, request: &PolicyRequest) -> bool {
        match condition {
            Condition::Host(pattern) => {
                if let Some(host) = &request.host {
                    pattern_matches(pattern, host)
                } else {
                    false
                }
            }
            Condition::Path(pattern) => {
                if let Some(path) = &request.path {
                    pattern_matches(pattern, path)
                } else {
                    false
                }
            }
            Condition::Port(port) => request.port == Some(*port),
            Condition::Protocol(proto) => request.protocol.as_deref() == Some(proto.as_str()),
            Condition::Operation(op) => request.operation.as_deref() == Some(op.as_str()),
            _ => true, // Other conditions not evaluated here
        }
    }

    /// Get all policies.
    pub fn policies(&self) -> &[NaturalPolicy] {
        &self.policies
    }

    /// Get policy by ID.
    pub fn get_policy(&self, id: &str) -> Option<&NaturalPolicy> {
        self.policies.iter().find(|p| p.id == id)
    }
}

/// Policy request to evaluate.
#[derive(Debug, Clone, Default)]
pub struct PolicyRequest {
    /// Resource kind.
    pub resource: ResourceKind,
    /// Target host.
    pub host: Option<String>,
    /// Target path.
    pub path: Option<String>,
    /// Target port.
    pub port: Option<u16>,
    /// Protocol.
    pub protocol: Option<String>,
    /// Operation type.
    pub operation: Option<String>,
}

impl Default for ResourceKind {
    fn default() -> Self {
        Self::All
    }
}

impl PolicyRequest {
    /// Create a network request.
    pub fn network(host: &str, port: u16) -> Self {
        Self {
            resource: ResourceKind::Network,
            host: Some(host.to_string()),
            port: Some(port),
            ..Default::default()
        }
    }

    /// Create a filesystem request.
    pub fn filesystem(path: &str, operation: &str) -> Self {
        Self {
            resource: ResourceKind::FileSystem,
            path: Some(path.to_string()),
            operation: Some(operation.to_string()),
            ..Default::default()
        }
    }
}

/// Parse error.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// Empty input.
    EmptyInput,
    /// No action found.
    NoActionFound,
    /// Ambiguous policy.
    Ambiguous(String),
    /// Invalid syntax.
    InvalidSyntax(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "Empty input"),
            Self::NoActionFound => write!(f, "No action keyword found"),
            Self::Ambiguous(msg) => write!(f, "Ambiguous policy: {}", msg),
            Self::InvalidSyntax(msg) => write!(f, "Invalid syntax: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

fn generate_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    format!("policy-{:016x}", hasher.finish())
}

fn extract_number(text: &str) -> Option<u64> {
    for word in text.split_whitespace() {
        if let Ok(n) = word.parse::<u64>() {
            return Some(n);
        }
    }
    None
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            if parts[0].is_empty() {
                // *suffix
                value.ends_with(parts[1])
            } else if parts[1].is_empty() {
                // prefix*
                value.starts_with(parts[0])
            } else {
                // prefix*suffix
                value.starts_with(parts[0]) && value.ends_with(parts[1])
            }
        } else {
            value.contains(&pattern.replace('*', ""))
        }
    } else {
        value == pattern || value.contains(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let parser = PolicyParser::default();
        assert!(!parser.keywords.is_empty());
    }

    #[test]
    fn test_parse_allow_network() {
        let parser = PolicyParser::default();
        let policy = parser.parse("allow network access to api.example.com").unwrap();

        assert_eq!(policy.rules.len(), 1);
        assert_eq!(policy.rules[0].action, PolicyAction::Allow);
        assert_eq!(policy.rules[0].resource, ResourceKind::Network);
    }

    #[test]
    fn test_parse_deny_files() {
        let parser = PolicyParser::default();
        let policy = parser.parse("deny file writes except to /tmp").unwrap();

        assert_eq!(policy.rules[0].action, PolicyAction::Deny);
        assert_eq!(policy.rules[0].resource, ResourceKind::FileSystem);
    }

    #[test]
    fn test_parse_block_process() {
        let parser = PolicyParser::default();
        let policy = parser.parse("block process execution").unwrap();

        assert_eq!(policy.rules[0].action, PolicyAction::Deny);
        assert_eq!(policy.rules[0].resource, ResourceKind::Process);
    }

    #[test]
    fn test_extract_host_condition() {
        let parser = PolicyParser::default();
        let policy = parser.parse("allow network to api.example.com").unwrap();

        let has_host = policy.rules[0]
            .conditions
            .iter()
            .any(|c| matches!(c, Condition::Host(h) if h == "api.example.com"));
        assert!(has_host);
    }

    #[test]
    fn test_extract_path_condition() {
        let parser = PolicyParser::default();
        let policy = parser.parse("allow file access to /tmp/data").unwrap();

        let has_path = policy.rules[0]
            .conditions
            .iter()
            .any(|c| matches!(c, Condition::Path(p) if p == "/tmp/data"));
        assert!(has_path);
    }

    #[test]
    fn test_policy_engine_add() {
        let mut engine = PolicyEngine::new();
        let id = engine.add_policy("allow network access").unwrap();
        assert!(!id.is_empty());
        assert_eq!(engine.policies().len(), 1);
    }

    #[test]
    fn test_policy_engine_evaluate() {
        let mut engine = PolicyEngine::new();
        engine.add_policy("allow network access to api.example.com").unwrap();
        engine.add_policy("deny network access to evil.com").unwrap();

        let request = PolicyRequest::network("api.example.com", 443);
        let decision = engine.evaluate(&request);

        assert_eq!(decision.action, PolicyAction::Allow);
    }

    #[test]
    fn test_policy_engine_default_deny() {
        let engine = PolicyEngine::new();
        let request = PolicyRequest::network("unknown.com", 80);
        let decision = engine.evaluate(&request);

        assert_eq!(decision.action, PolicyAction::Deny);
    }

    #[test]
    fn test_policy_enable_disable() {
        let mut engine = PolicyEngine::new();
        let id = engine.add_policy("allow network access").unwrap();

        engine.set_enabled(&id, false);
        let policy = engine.get_policy(&id).unwrap();
        assert!(!policy.enabled);
    }

    #[test]
    fn test_pattern_matching() {
        assert!(pattern_matches("*.example.com", "api.example.com"));
        assert!(pattern_matches("/tmp/*", "/tmp/data"));
        assert!(pattern_matches("exact", "exact"));
        assert!(!pattern_matches("*.other.com", "api.example.com"));
    }

    #[test]
    fn test_filesystem_request() {
        let request = PolicyRequest::filesystem("/tmp/test.txt", "write");
        assert_eq!(request.resource, ResourceKind::FileSystem);
        assert_eq!(request.path, Some("/tmp/test.txt".to_string()));
    }

    #[test]
    fn test_audit_action() {
        let parser = PolicyParser::default();
        let policy = parser.parse("audit all file access").unwrap();

        assert_eq!(policy.rules[0].action, PolicyAction::Audit);
    }

    #[test]
    fn test_rate_limit_action() {
        let parser = PolicyParser::default();
        let policy = parser.parse("limit network requests to 100 per second").unwrap();

        assert_eq!(policy.rules[0].action, PolicyAction::RateLimit);
    }

    #[test]
    fn test_remove_policy() {
        let mut engine = PolicyEngine::new();
        let id = engine.add_policy("allow network access").unwrap();

        assert!(engine.remove_policy(&id));
        assert!(engine.policies().is_empty());
    }

    #[test]
    fn test_empty_input_error() {
        let parser = PolicyParser::default();
        let result = parser.parse("");
        assert!(matches!(result, Err(ParseError::EmptyInput)));
    }
}
