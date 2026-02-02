//! Budget Alerts & Notifications
//!
//! Alert system for cost and carbon budgets with cooldown support.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Alert trigger conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertTrigger {
    CostExceeded { threshold_usd: f64 },
    CarbonExceeded { threshold_grams: f64 },
    BudgetPercentage { percent: f64 },
    RateSpike { multiplier: f64 },
    RegionHighIntensity { region: String, threshold_gco2: f64 },
}

/// An alert that was triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub severity: AlertSeverity,
    pub trigger: AlertTrigger,
    pub message: String,
    pub triggered_at: SystemTime,
    pub acknowledged: bool,
}

/// Alert rule definition.
pub struct AlertRule {
    pub name: String,
    pub trigger: AlertTrigger,
    pub severity: AlertSeverity,
    pub cooldown: Duration,
    pub enabled: bool,
}

/// Alert manager tracks rules and fires alerts.
pub struct AlertManager {
    rules: Vec<AlertRule>,
    alerts: Vec<Alert>,
    last_fired: HashMap<String, SystemTime>,
    counter: u64,
}

impl AlertManager {
    /// Create a new empty alert manager.
    pub fn new() -> Self {
        Self { rules: Vec::new(), alerts: Vec::new(), last_fired: HashMap::new(), counter: 0 }
    }

    /// Add an alert rule.
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    /// Remove an alert rule by name.
    pub fn remove_rule(&mut self, name: &str) {
        self.rules.retain(|r| r.name != name);
    }

    /// Check cost against all cost-related rules, returning any triggered alerts.
    pub fn check_cost(&mut self, cost_usd: f64) -> Vec<Alert> {
        let now = SystemTime::now();

        let matched: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .filter_map(|rule| {
                if let AlertTrigger::CostExceeded { threshold_usd } = &rule.trigger {
                    if cost_usd >= *threshold_usd && self.can_fire(&rule.name, rule.cooldown, now) {
                        return Some((
                            rule.name.clone(),
                            rule.severity,
                            rule.trigger.clone(),
                            format!(
                                "Cost ${:.4} exceeded threshold ${:.4}",
                                cost_usd, threshold_usd
                            ),
                        ));
                    }
                }
                None
            })
            .collect();

        let mut fired = Vec::new();
        for (name, severity, trigger, message) in matched {
            let alert = self.create_alert(severity, trigger, message, now);
            self.last_fired.insert(name, now);
            fired.push(alert);
        }

        self.alerts.extend(fired.clone());
        fired
    }

    /// Check carbon usage against all carbon-related rules, returning any triggered alerts.
    pub fn check_carbon(&mut self, carbon_grams: f64) -> Vec<Alert> {
        let now = SystemTime::now();

        let matched: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .filter_map(|rule| {
                if let AlertTrigger::CarbonExceeded { threshold_grams } = &rule.trigger {
                    if carbon_grams >= *threshold_grams
                        && self.can_fire(&rule.name, rule.cooldown, now)
                    {
                        return Some((
                            rule.name.clone(),
                            rule.severity,
                            rule.trigger.clone(),
                            format!(
                                "Carbon {:.2}g exceeded threshold {:.2}g",
                                carbon_grams, threshold_grams
                            ),
                        ));
                    }
                }
                None
            })
            .collect();

        let mut fired = Vec::new();
        for (name, severity, trigger, message) in matched {
            let alert = self.create_alert(severity, trigger, message, now);
            self.last_fired.insert(name, now);
            fired.push(alert);
        }

        self.alerts.extend(fired.clone());
        fired
    }

    /// Check budget usage percentage against budget percentage rules.
    pub fn check_budget_usage(&mut self, used: f64, total: f64) -> Vec<Alert> {
        let now = SystemTime::now();

        if total <= 0.0 {
            return Vec::new();
        }

        let usage_pct = used / total;

        let matched: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .filter_map(|rule| {
                if let AlertTrigger::BudgetPercentage { percent } = &rule.trigger {
                    if usage_pct >= *percent && self.can_fire(&rule.name, rule.cooldown, now) {
                        return Some((
                            rule.name.clone(),
                            rule.severity,
                            rule.trigger.clone(),
                            format!(
                                "Budget usage {:.1}% exceeded threshold {:.1}%",
                                usage_pct * 100.0,
                                percent * 100.0
                            ),
                        ));
                    }
                }
                None
            })
            .collect();

        let mut fired = Vec::new();
        for (name, severity, trigger, message) in matched {
            let alert = self.create_alert(severity, trigger, message, now);
            self.last_fired.insert(name, now);
            fired.push(alert);
        }

        self.alerts.extend(fired.clone());
        fired
    }

    /// Acknowledge an alert by ID.
    pub fn acknowledge(&mut self, alert_id: &str) {
        for alert in &mut self.alerts {
            if alert.id == alert_id {
                alert.acknowledged = true;
            }
        }
    }

    /// Get all unacknowledged alerts.
    pub fn active_alerts(&self) -> Vec<&Alert> {
        self.alerts.iter().filter(|a| !a.acknowledged).collect()
    }

    /// Get full alert history.
    pub fn alert_history(&self) -> &[Alert] {
        &self.alerts
    }

    fn can_fire(&self, rule_name: &str, cooldown: Duration, now: SystemTime) -> bool {
        match self.last_fired.get(rule_name) {
            Some(last) => {
                now.duration_since(*last).map(|elapsed| elapsed >= cooldown).unwrap_or(true)
            }
            None => true,
        }
    }

    fn create_alert(
        &mut self,
        severity: AlertSeverity,
        trigger: AlertTrigger,
        message: String,
        triggered_at: SystemTime,
    ) -> Alert {
        self.counter += 1;
        Alert {
            id: format!("alert-{}", self.counter),
            severity,
            trigger,
            message,
            triggered_at,
            acknowledged: false,
        }
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost_rule(name: &str, threshold: f64, severity: AlertSeverity) -> AlertRule {
        AlertRule {
            name: name.to_string(),
            trigger: AlertTrigger::CostExceeded { threshold_usd: threshold },
            severity,
            cooldown: Duration::from_secs(0),
            enabled: true,
        }
    }

    #[test]
    fn test_cost_alert_fires() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(cost_rule("high-cost", 10.0, AlertSeverity::Warning));
        let alerts = mgr.check_cost(15.0);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AlertSeverity::Warning);
    }

    #[test]
    fn test_cost_alert_not_fired_below_threshold() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(cost_rule("high-cost", 10.0, AlertSeverity::Warning));
        let alerts = mgr.check_cost(5.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_carbon_alert_fires() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(AlertRule {
            name: "carbon-limit".to_string(),
            trigger: AlertTrigger::CarbonExceeded { threshold_grams: 100.0 },
            severity: AlertSeverity::Critical,
            cooldown: Duration::from_secs(0),
            enabled: true,
        });
        let alerts = mgr.check_carbon(150.0);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AlertSeverity::Critical);
    }

    #[test]
    fn test_budget_percentage_alert() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(AlertRule {
            name: "budget-80".to_string(),
            trigger: AlertTrigger::BudgetPercentage { percent: 0.8 },
            severity: AlertSeverity::Warning,
            cooldown: Duration::from_secs(0),
            enabled: true,
        });
        let alerts = mgr.check_budget_usage(85.0, 100.0);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].message.contains("85.0%"));
    }

    #[test]
    fn test_budget_zero_total_no_alert() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(AlertRule {
            name: "budget-80".to_string(),
            trigger: AlertTrigger::BudgetPercentage { percent: 0.8 },
            severity: AlertSeverity::Warning,
            cooldown: Duration::from_secs(0),
            enabled: true,
        });
        let alerts = mgr.check_budget_usage(85.0, 0.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_acknowledge_alert() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(cost_rule("high-cost", 1.0, AlertSeverity::Info));
        let alerts = mgr.check_cost(5.0);
        assert_eq!(mgr.active_alerts().len(), 1);
        mgr.acknowledge(&alerts[0].id);
        assert_eq!(mgr.active_alerts().len(), 0);
    }

    #[test]
    fn test_alert_history() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(cost_rule("cost-warn", 1.0, AlertSeverity::Warning));
        mgr.check_cost(2.0);
        mgr.check_cost(3.0);
        assert_eq!(mgr.alert_history().len(), 2);
    }

    #[test]
    fn test_disabled_rule_not_fired() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(AlertRule {
            name: "disabled".to_string(),
            trigger: AlertTrigger::CostExceeded { threshold_usd: 1.0 },
            severity: AlertSeverity::Info,
            cooldown: Duration::from_secs(0),
            enabled: false,
        });
        let alerts = mgr.check_cost(100.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_remove_rule() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(cost_rule("to-remove", 1.0, AlertSeverity::Info));
        mgr.remove_rule("to-remove");
        let alerts = mgr.check_cost(100.0);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_cooldown_prevents_rapid_firing() {
        let mut mgr = AlertManager::new();
        mgr.add_rule(AlertRule {
            name: "cooldown-rule".to_string(),
            trigger: AlertTrigger::CostExceeded { threshold_usd: 1.0 },
            severity: AlertSeverity::Warning,
            cooldown: Duration::from_secs(3600), // 1 hour cooldown
            enabled: true,
        });
        let first = mgr.check_cost(5.0);
        assert_eq!(first.len(), 1);
        // Second check should be suppressed by cooldown
        let second = mgr.check_cost(10.0);
        assert!(second.is_empty());
    }
}
