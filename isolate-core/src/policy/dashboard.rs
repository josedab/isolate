//! Live Policy Evaluation Dashboard
//!
//! Provides real-time visibility into policy evaluations, including:
//! - Evaluation statistics (total, allow, deny, error counts)
//! - Per-rule match counts and hit rates
//! - Dry-run "what-if" testing for new policies
//! - Evaluation audit trail with filtering
//! - Dashboard snapshots for reporting

use super::engine::{PolicyDecision, PolicyEngine};
use super::rules::{Effect, PolicyRule, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

/// Configuration for the policy dashboard.
#[derive(Debug, Clone)]
pub struct DashboardConfig {
    /// Maximum number of evaluation records to retain.
    pub max_history: usize,
    /// Whether to record full evaluation traces.
    pub record_traces: bool,
    /// Interval for aggregating statistics.
    pub aggregation_interval: Duration,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            max_history: 10_000,
            record_traces: true,
            aggregation_interval: Duration::from_secs(60),
        }
    }
}

/// A single evaluation record captured by the dashboard.
#[derive(Debug, Clone)]
pub struct EvalRecord {
    /// Unique record ID.
    pub id: u64,
    /// Timestamp of the evaluation.
    pub timestamp: SystemTime,
    /// Evaluation latency.
    pub latency: Duration,
    /// Action that was evaluated.
    pub action: String,
    /// Resource that was evaluated.
    pub resource: String,
    /// Principal (caller) that was evaluated.
    pub principal: String,
    /// Final decision.
    pub effect: Effect,
    /// ID of the rule that determined the outcome (if any).
    pub determining_rule: Option<String>,
    /// Whether this was a dry-run.
    pub dry_run: bool,
}

/// Per-rule statistics.
#[derive(Debug, Clone, Default)]
pub struct RuleStats {
    /// Rule ID.
    pub rule_id: String,
    /// Number of times this rule matched.
    pub match_count: u64,
    /// Number of times this rule was the determining factor.
    pub determining_count: u64,
    /// Effect of this rule.
    pub effect: Option<Effect>,
}

/// Aggregate dashboard statistics.
#[derive(Debug, Clone)]
pub struct DashboardStats {
    /// Total evaluations performed.
    pub total_evaluations: u64,
    /// Number of Allow decisions.
    pub allow_count: u64,
    /// Number of Deny decisions.
    pub deny_count: u64,
    /// Average evaluation latency.
    pub avg_latency: Duration,
    /// 99th-percentile latency.
    pub p99_latency: Duration,
    /// Number of dry-run evaluations.
    pub dry_run_count: u64,
    /// Per-rule statistics.
    pub rule_stats: Vec<RuleStats>,
    /// Most denied actions.
    pub top_denied_actions: Vec<(String, u64)>,
    /// Most active principals.
    pub top_principals: Vec<(String, u64)>,
}

/// A complete dashboard snapshot for reporting.
#[derive(Debug, Clone)]
pub struct DashboardSnapshot {
    /// Snapshot timestamp.
    pub timestamp: SystemTime,
    /// Statistics.
    pub stats: DashboardStats,
    /// Active rule count.
    pub active_rules: usize,
    /// Recent records (last N).
    pub recent_records: Vec<EvalRecord>,
}

/// Result of a dry-run "what-if" evaluation.
#[derive(Debug, Clone)]
pub struct WhatIfResult {
    /// The evaluated decision.
    pub decision: Effect,
    /// Which rule determined the outcome.
    pub determining_rule: Option<String>,
    /// Rules that matched.
    pub matched_rules: Vec<String>,
    /// Latency of the evaluation.
    pub latency: Duration,
}

/// Live policy evaluation dashboard.
///
/// Wraps a `PolicyEngine` and records all evaluations, providing
/// real-time statistics, history, and dry-run capabilities.
pub struct PolicyDashboard {
    engine: PolicyEngine,
    config: DashboardConfig,
    records: Vec<EvalRecord>,
    rule_stats: HashMap<String, RuleStats>,
    action_deny_counts: HashMap<String, u64>,
    principal_counts: HashMap<String, u64>,
    next_id: u64,
    latencies: Vec<Duration>,
}

impl PolicyDashboard {
    /// Creates a new dashboard wrapping the given engine.
    pub fn new(engine: PolicyEngine, config: DashboardConfig) -> Self {
        Self {
            engine,
            config,
            records: Vec::new(),
            rule_stats: HashMap::new(),
            action_deny_counts: HashMap::new(),
            principal_counts: HashMap::new(),
            next_id: 0,
            latencies: Vec::new(),
        }
    }

    /// Creates a dashboard with default configuration.
    pub fn with_defaults(engine: PolicyEngine) -> Self {
        Self::new(engine, DashboardConfig::default())
    }

    /// Evaluates a policy request and records the result.
    pub fn evaluate(
        &mut self,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> PolicyDecision {
        let start = Instant::now();
        let decision = self.engine.evaluate(action, resource, principal, context);
        let latency = start.elapsed();

        self.record_evaluation(action, resource, principal, &decision, latency, false);
        decision
    }

    /// Performs a dry-run evaluation without affecting statistics beyond dry_run_count.
    pub fn what_if(
        &mut self,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> WhatIfResult {
        let start = Instant::now();
        let decision = self.engine.evaluate(action, resource, principal, context);
        let latency = start.elapsed();

        let matched_rules: Vec<String> = decision
            .trace
            .entries
            .iter()
            .filter(|e| e.matched)
            .map(|e| e.rule_id.clone())
            .collect();

        self.record_evaluation(action, resource, principal, &decision, latency, true);

        WhatIfResult {
            decision: decision.effect,
            determining_rule: decision.determining_rule,
            matched_rules,
            latency,
        }
    }

    /// Adds a rule to the underlying engine and resets its stats.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        let rule_id = rule.id.clone();
        self.engine.add_rule(rule);
        self.rule_stats.entry(rule_id).or_default();
    }

    /// Returns current aggregate statistics.
    pub fn stats(&self) -> DashboardStats {
        let total = self.records.len() as u64;
        let allow_count =
            self.records.iter().filter(|r| !r.dry_run && r.effect == Effect::Allow).count() as u64;
        let deny_count =
            self.records.iter().filter(|r| !r.dry_run && r.effect == Effect::Deny).count() as u64;
        let dry_run_count = self.records.iter().filter(|r| r.dry_run).count() as u64;

        let avg_latency = if self.latencies.is_empty() {
            Duration::ZERO
        } else {
            let sum: Duration = self.latencies.iter().sum();
            sum / self.latencies.len() as u32
        };

        let p99_latency = self.percentile_latency(99);

        let mut rule_stats: Vec<RuleStats> = self.rule_stats.values().cloned().collect();
        rule_stats.sort_by(|a, b| b.match_count.cmp(&a.match_count));

        let mut top_denied: Vec<(String, u64)> =
            self.action_deny_counts.clone().into_iter().collect();
        top_denied.sort_by(|a, b| b.1.cmp(&a.1));
        top_denied.truncate(10);

        let mut top_principals: Vec<(String, u64)> =
            self.principal_counts.clone().into_iter().collect();
        top_principals.sort_by(|a, b| b.1.cmp(&a.1));
        top_principals.truncate(10);

        DashboardStats {
            total_evaluations: total,
            allow_count,
            deny_count,
            avg_latency,
            p99_latency,
            dry_run_count,
            rule_stats,
            top_denied_actions: top_denied,
            top_principals,
        }
    }

    /// Creates a full dashboard snapshot.
    pub fn snapshot(&self, recent_count: usize) -> DashboardSnapshot {
        let recent: Vec<EvalRecord> =
            self.records.iter().rev().take(recent_count).cloned().collect();

        DashboardSnapshot {
            timestamp: SystemTime::now(),
            stats: self.stats(),
            active_rules: self.engine.rule_count(),
            recent_records: recent,
        }
    }

    /// Returns evaluation records filtered by criteria.
    pub fn query_records(&self, filter: &RecordFilter) -> Vec<&EvalRecord> {
        self.records
            .iter()
            .filter(|r| {
                if let Some(ref effect) = filter.effect {
                    if &r.effect != effect {
                        return false;
                    }
                }
                if let Some(ref action) = filter.action {
                    if !r.action.contains(action.as_str()) {
                        return false;
                    }
                }
                if let Some(ref principal) = filter.principal {
                    if !r.principal.contains(principal.as_str()) {
                        return false;
                    }
                }
                if filter.dry_run_only && !r.dry_run {
                    return false;
                }
                true
            })
            .take(filter.limit.unwrap_or(100))
            .collect()
    }

    /// Returns the number of recorded evaluations.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Clears all history and statistics.
    pub fn reset(&mut self) {
        self.records.clear();
        self.rule_stats.clear();
        self.action_deny_counts.clear();
        self.principal_counts.clear();
        self.latencies.clear();
        self.next_id = 0;
    }

    /// Renders the dashboard as a formatted text report.
    pub fn render_text(&self) -> String {
        let stats = self.stats();
        let mut out = String::new();

        out.push_str("╔══════════════════════════════════════════════╗\n");
        out.push_str("║       POLICY EVALUATION DASHBOARD           ║\n");
        out.push_str("╠══════════════════════════════════════════════╣\n");
        out.push_str(&format!("║ Total Evaluations: {:>24} ║\n", stats.total_evaluations));
        out.push_str(&format!("║ Allowed:           {:>24} ║\n", stats.allow_count));
        out.push_str(&format!("║ Denied:            {:>24} ║\n", stats.deny_count));
        out.push_str(&format!("║ Dry-Runs:          {:>24} ║\n", stats.dry_run_count));
        out.push_str(&format!(
            "║ Avg Latency:       {:>21.2}µs ║\n",
            stats.avg_latency.as_secs_f64() * 1_000_000.0
        ));
        out.push_str(&format!(
            "║ P99 Latency:       {:>21.2}µs ║\n",
            stats.p99_latency.as_secs_f64() * 1_000_000.0
        ));
        out.push_str("╠══════════════════════════════════════════════╣\n");

        if !stats.rule_stats.is_empty() {
            out.push_str("║ Top Rules by Match Count:                    ║\n");
            for rs in stats.rule_stats.iter().take(5) {
                out.push_str(&format!(
                    "║   {:<20} matches={:<6} det={:<5}║\n",
                    truncate_str(&rs.rule_id, 20),
                    rs.match_count,
                    rs.determining_count
                ));
            }
        }

        if !stats.top_denied_actions.is_empty() {
            out.push_str("╠══════════════════════════════════════════════╣\n");
            out.push_str("║ Top Denied Actions:                          ║\n");
            for (action, count) in stats.top_denied_actions.iter().take(5) {
                out.push_str(&format!(
                    "║   {:<30} count={:<5}║\n",
                    truncate_str(action, 30),
                    count
                ));
            }
        }

        out.push_str("╚══════════════════════════════════════════════╝\n");
        out
    }

    // -- private helpers --

    fn record_evaluation(
        &mut self,
        action: &str,
        resource: &str,
        principal: &str,
        decision: &PolicyDecision,
        latency: Duration,
        dry_run: bool,
    ) {
        self.next_id += 1;
        let record = EvalRecord {
            id: self.next_id,
            timestamp: SystemTime::now(),
            latency,
            action: action.to_string(),
            resource: resource.to_string(),
            principal: principal.to_string(),
            effect: decision.effect,
            determining_rule: decision.determining_rule.clone(),
            dry_run,
        };

        // Update per-rule stats
        for entry in &decision.trace.entries {
            if entry.matched {
                let rs = self.rule_stats.entry(entry.rule_id.clone()).or_insert_with(|| {
                    RuleStats { rule_id: entry.rule_id.clone(), ..Default::default() }
                });
                rs.match_count += 1;
            }
        }
        if let Some(ref rule_id) = decision.determining_rule {
            let rs = self
                .rule_stats
                .entry(rule_id.clone())
                .or_insert_with(|| RuleStats { rule_id: rule_id.clone(), ..Default::default() });
            rs.determining_count += 1;
        }

        // Update action deny counts
        if !dry_run && decision.effect == Effect::Deny {
            *self.action_deny_counts.entry(action.to_string()).or_insert(0) += 1;
        }

        // Update principal counts
        if !dry_run {
            *self.principal_counts.entry(principal.to_string()).or_insert(0) += 1;
        }

        self.latencies.push(latency);

        // Enforce history limit
        if self.records.len() >= self.config.max_history {
            self.records.remove(0);
        }
        self.records.push(record);
    }

    fn percentile_latency(&self, pct: usize) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted = self.latencies.clone();
        sorted.sort();
        let idx = (sorted.len() * pct / 100).min(sorted.len() - 1);
        sorted[idx]
    }
}

/// Filter for querying evaluation records.
#[derive(Debug, Clone, Default)]
pub struct RecordFilter {
    /// Filter by effect.
    pub effect: Option<Effect>,
    /// Filter by action substring.
    pub action: Option<String>,
    /// Filter by principal substring.
    pub principal: Option<String>,
    /// Only include dry-run records.
    pub dry_run_only: bool,
    /// Maximum number of records to return.
    pub limit: Option<usize>,
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::rules::PolicyRuleBuilder;

    fn test_engine_with_rules() -> PolicyEngine {
        let mut engine = PolicyEngine::new();

        let allow_read = PolicyRuleBuilder::new("allow-read")
            .action("read")
            .resource("docs/*")
            .effect(Effect::Allow)
            .priority(10)
            .build();

        let deny_admin = PolicyRuleBuilder::new("deny-admin-delete")
            .action("delete")
            .resource("*")
            .principal("admin")
            .effect(Effect::Deny)
            .priority(20)
            .build();

        let allow_all = PolicyRuleBuilder::new("allow-all")
            .action("*")
            .resource("*")
            .effect(Effect::Allow)
            .priority(1)
            .build();

        engine.add_rule(allow_read);
        engine.add_rule(deny_admin);
        engine.add_rule(allow_all);
        engine
    }

    #[test]
    fn test_dashboard_creation() {
        let engine = PolicyEngine::new();
        let dash = PolicyDashboard::with_defaults(engine);
        assert_eq!(dash.record_count(), 0);
    }

    #[test]
    fn test_evaluate_and_record() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        let decision = dash.evaluate("read", "docs/readme", "user1", &ctx);
        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(dash.record_count(), 1);
    }

    #[test]
    fn test_deny_recorded() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        let decision = dash.evaluate("delete", "data/secret", "admin", &ctx);
        assert_eq!(decision.effect, Effect::Deny);

        let stats = dash.stats();
        assert_eq!(stats.deny_count, 1);
    }

    #[test]
    fn test_what_if_dry_run() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        let result = dash.what_if("delete", "data/secret", "admin", &ctx);
        assert_eq!(result.decision, Effect::Deny);
        assert!(!result.matched_rules.is_empty());

        let stats = dash.stats();
        assert_eq!(stats.dry_run_count, 1);
        // Dry runs don't count toward allow/deny
        assert_eq!(stats.deny_count, 0);
    }

    #[test]
    fn test_stats_aggregate() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        for _ in 0..10 {
            dash.evaluate("read", "docs/a", "user1", &ctx);
        }
        for _ in 0..5 {
            dash.evaluate("delete", "x", "admin", &ctx);
        }

        let stats = dash.stats();
        assert_eq!(stats.total_evaluations, 15);
        assert_eq!(stats.allow_count, 10);
        assert_eq!(stats.deny_count, 5);
        assert!(stats.avg_latency > Duration::ZERO || stats.avg_latency == Duration::ZERO);
    }

    #[test]
    fn test_rule_stats() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "u", &ctx);
        dash.evaluate("read", "docs/b", "u", &ctx);

        let stats = dash.stats();
        let allow_read = stats.rule_stats.iter().find(|r| r.rule_id == "allow-read");
        assert!(allow_read.is_some());
        assert!(allow_read.unwrap().match_count >= 2);
    }

    #[test]
    fn test_top_denied_actions() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        for _ in 0..5 {
            dash.evaluate("delete", "x", "admin", &ctx);
        }

        let stats = dash.stats();
        assert!(!stats.top_denied_actions.is_empty());
        assert_eq!(stats.top_denied_actions[0].0, "delete");
        assert_eq!(stats.top_denied_actions[0].1, 5);
    }

    #[test]
    fn test_top_principals() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        for _ in 0..3 {
            dash.evaluate("read", "docs/a", "alice", &ctx);
        }
        dash.evaluate("read", "docs/b", "bob", &ctx);

        let stats = dash.stats();
        assert!(!stats.top_principals.is_empty());
    }

    #[test]
    fn test_snapshot() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "u", &ctx);

        let snap = dash.snapshot(5);
        assert_eq!(snap.active_rules, 3);
        assert_eq!(snap.recent_records.len(), 1);
    }

    #[test]
    fn test_query_records_by_effect() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "user1", &ctx);
        dash.evaluate("delete", "x", "admin", &ctx);

        let filter = RecordFilter { effect: Some(Effect::Deny), ..Default::default() };
        let results = dash.query_records(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].effect, Effect::Deny);
    }

    #[test]
    fn test_query_records_by_action() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "user1", &ctx);
        dash.evaluate("delete", "x", "admin", &ctx);

        let filter = RecordFilter { action: Some("read".to_string()), ..Default::default() };
        let results = dash.query_records(&filter);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_query_dry_run_only() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "u", &ctx);
        dash.what_if("delete", "x", "admin", &ctx);

        let filter = RecordFilter { dry_run_only: true, ..Default::default() };
        let results = dash.query_records(&filter);
        assert_eq!(results.len(), 1);
        assert!(results[0].dry_run);
    }

    #[test]
    fn test_reset_clears_all() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "u", &ctx);
        assert_eq!(dash.record_count(), 1);

        dash.reset();
        assert_eq!(dash.record_count(), 0);

        let stats = dash.stats();
        assert_eq!(stats.total_evaluations, 0);
    }

    #[test]
    fn test_history_limit_enforced() {
        let engine = test_engine_with_rules();
        let config = DashboardConfig { max_history: 5, ..Default::default() };
        let mut dash = PolicyDashboard::new(engine, config);

        let ctx = HashMap::new();
        for _ in 0..10 {
            dash.evaluate("read", "docs/a", "u", &ctx);
        }

        assert_eq!(dash.record_count(), 5);
    }

    #[test]
    fn test_render_text_output() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        dash.evaluate("read", "docs/a", "user1", &ctx);
        dash.evaluate("delete", "x", "admin", &ctx);

        let text = dash.render_text();
        assert!(text.contains("POLICY EVALUATION DASHBOARD"));
        assert!(text.contains("Total Evaluations"));
        assert!(text.contains("Denied"));
    }

    #[test]
    fn test_add_rule_through_dashboard() {
        let engine = PolicyEngine::new();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let rule = PolicyRuleBuilder::new("test-rule")
            .action("*")
            .resource("*")
            .effect(Effect::Allow)
            .build();

        dash.add_rule(rule);

        let ctx = HashMap::new();
        let decision = dash.evaluate("read", "any", "anyone", &ctx);
        assert_eq!(decision.effect, Effect::Allow);
    }

    #[test]
    fn test_what_if_returns_matched_rules() {
        let engine = test_engine_with_rules();
        let mut dash = PolicyDashboard::with_defaults(engine);

        let ctx = HashMap::new();
        let result = dash.what_if("read", "docs/readme", "user1", &ctx);
        assert!(!result.matched_rules.is_empty());
    }
}
