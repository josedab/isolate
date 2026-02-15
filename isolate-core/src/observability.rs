//! Observability dashboard configuration, Prometheus alert rules, and Grafana dashboard templates.
//!
//! This module provides structured types and generators for monitoring dashboards
//! and alert rules. It references metric names from the [`crate::metrics`] module.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Panel types
// ---------------------------------------------------------------------------

/// Grafana panel visualization type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PanelType {
    Graph,
    Stat,
    Gauge,
    Table,
    Heatmap,
}

impl fmt::Display for PanelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PanelType::Graph => write!(f, "graph"),
            PanelType::Stat => write!(f, "stat"),
            PanelType::Gauge => write!(f, "gauge"),
            PanelType::Table => write!(f, "table"),
            PanelType::Heatmap => write!(f, "heatmap"),
        }
    }
}

// ---------------------------------------------------------------------------
// Threshold
// ---------------------------------------------------------------------------

/// A visual threshold for a Grafana panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub value: f64,
    pub color: String,
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// GrafanaPanel
// ---------------------------------------------------------------------------

/// A single panel inside a Grafana dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaPanel {
    pub id: u32,
    pub title: String,
    pub panel_type: PanelType,
    /// PromQL query powering the panel.
    pub query: String,
    pub description: Option<String>,
    /// Unit hint (e.g. `"s"`, `"bytes"`, `"ops"`).
    pub unit: Option<String>,
    pub thresholds: Vec<Threshold>,
}

// ---------------------------------------------------------------------------
// GrafanaDashboard
// ---------------------------------------------------------------------------

/// A Grafana dashboard template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaDashboard {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub panels: Vec<GrafanaPanel>,
    /// Auto-refresh interval (e.g. `"10s"`).
    pub refresh: String,
    /// Dashboard time range start (e.g. `"now-1h"`).
    pub time_from: String,
    /// Dashboard time range end (e.g. `"now"`).
    pub time_to: String,
}

// ---------------------------------------------------------------------------
// Alert severity
// ---------------------------------------------------------------------------

/// Severity level for a Prometheus alert rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "info"),
            AlertSeverity::Warning => write!(f, "warning"),
            AlertSeverity::Critical => write!(f, "critical"),
        }
    }
}

// ---------------------------------------------------------------------------
// PrometheusAlert
// ---------------------------------------------------------------------------

/// A single Prometheus alerting rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusAlert {
    pub name: String,
    /// PromQL expression that triggers the alert.
    pub expr: String,
    /// Duration the expression must be true (e.g. `"5m"`).
    pub duration: String,
    pub severity: AlertSeverity,
    pub summary: String,
    pub description: String,
    pub labels: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// AlertRuleGroup
// ---------------------------------------------------------------------------

/// A group of Prometheus alert rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleGroup {
    pub name: String,
    /// Evaluation interval (e.g. `"30s"`).
    pub interval: String,
    pub rules: Vec<PrometheusAlert>,
}

// ---------------------------------------------------------------------------
// ObservabilityConfig
// ---------------------------------------------------------------------------

/// Top-level observability configuration tying dashboards, alerts, and
/// scrape settings together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub dashboard: GrafanaDashboard,
    pub alerts: AlertRuleGroup,
    pub scrape_interval: String,
    pub metrics_path: String,
    pub metrics_port: u16,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            dashboard: default_dashboard(),
            alerts: default_alerts(),
            scrape_interval: "15s".to_string(),
            metrics_path: "/metrics".to_string(),
            metrics_port: 9090,
        }
    }
}

// ---------------------------------------------------------------------------
// Generator helpers
// ---------------------------------------------------------------------------

/// Returns a pre-built Grafana dashboard with panels covering the core Isolate
/// metrics defined in [`crate::metrics`].
pub fn default_dashboard() -> GrafanaDashboard {
    GrafanaDashboard {
        title: "Isolate Sandbox Runtime".to_string(),
        description: "Overview of sandbox lifecycle, resource usage, and error rates".to_string(),
        tags: vec!["isolate".to_string(), "wasm".to_string(), "sandbox".to_string()],
        refresh: "10s".to_string(),
        time_from: "now-1h".to_string(),
        time_to: "now".to_string(),
        panels: vec![
            GrafanaPanel {
                id: 1,
                title: "Sandbox Creation Rate".to_string(),
                panel_type: PanelType::Graph,
                query: "rate(isolate_sandboxes_created_total[5m])".to_string(),
                description: Some("Rate of new sandbox creation per second".to_string()),
                unit: Some("ops".to_string()),
                thresholds: vec![],
            },
            GrafanaPanel {
                id: 2,
                title: "Active Sandboxes".to_string(),
                panel_type: PanelType::Gauge,
                query: "isolate_sandboxes_active".to_string(),
                description: Some("Number of currently active sandboxes".to_string()),
                unit: None,
                thresholds: vec![
                    Threshold { value: 0.0, color: "green".to_string(), label: None },
                    Threshold { value: 80.0, color: "yellow".to_string(), label: Some("high".to_string()) },
                    Threshold { value: 95.0, color: "red".to_string(), label: Some("critical".to_string()) },
                ],
            },
            GrafanaPanel {
                id: 3,
                title: "Execution Duration (p50/p95/p99)".to_string(),
                panel_type: PanelType::Graph,
                query: "histogram_quantile(0.99, rate(isolate_sandbox_run_duration_seconds_bucket[5m]))".to_string(),
                description: Some("Sandbox execution latency percentiles".to_string()),
                unit: Some("s".to_string()),
                thresholds: vec![
                    Threshold { value: 1.0, color: "yellow".to_string(), label: Some("slow".to_string()) },
                    Threshold { value: 5.0, color: "red".to_string(), label: Some("very slow".to_string()) },
                ],
            },
            GrafanaPanel {
                id: 4,
                title: "Memory Usage".to_string(),
                panel_type: PanelType::Graph,
                query: "isolate_memory_bytes".to_string(),
                description: Some("Memory usage across sandboxes".to_string()),
                unit: Some("bytes".to_string()),
                thresholds: vec![],
            },
            GrafanaPanel {
                id: 5,
                title: "Error Rate".to_string(),
                panel_type: PanelType::Stat,
                query: r#"rate(isolate_sandbox_runs_total{status="failure"}[5m])"#.to_string(),
                description: Some("Rate of failed sandbox executions".to_string()),
                unit: Some("ops".to_string()),
                thresholds: vec![
                    Threshold { value: 0.0, color: "green".to_string(), label: None },
                    Threshold { value: 0.05, color: "red".to_string(), label: Some("high".to_string()) },
                ],
            },
            GrafanaPanel {
                id: 6,
                title: "Fuel Consumption Rate".to_string(),
                panel_type: PanelType::Graph,
                query: "rate(isolate_fuel_consumed_total[5m])".to_string(),
                description: Some("Rate of WASM fuel consumption".to_string()),
                unit: Some("ops".to_string()),
                thresholds: vec![],
            },
        ],
    }
}

/// Returns a pre-built set of Prometheus alert rules for Isolate.
pub fn default_alerts() -> AlertRuleGroup {
    AlertRuleGroup {
        name: "isolate-alerts".to_string(),
        interval: "30s".to_string(),
        rules: vec![
            PrometheusAlert {
                name: "HighErrorRate".to_string(),
                expr: r#"rate(isolate_sandbox_runs_total{status="failure"}[5m]) / rate(isolate_sandbox_runs_total[5m]) > 0.1"#.to_string(),
                duration: "5m".to_string(),
                severity: AlertSeverity::Critical,
                summary: "High sandbox error rate".to_string(),
                description: "More than 10% of sandbox executions are failing".to_string(),
                labels: HashMap::from([("component".to_string(), "sandbox".to_string())]),
            },
            PrometheusAlert {
                name: "HighLatency".to_string(),
                expr: "histogram_quantile(0.99, rate(isolate_sandbox_run_duration_seconds_bucket[5m])) > 5".to_string(),
                duration: "5m".to_string(),
                severity: AlertSeverity::Warning,
                summary: "High sandbox execution latency".to_string(),
                description: "p99 sandbox execution latency exceeds 5 seconds".to_string(),
                labels: HashMap::from([("component".to_string(), "sandbox".to_string())]),
            },
            PrometheusAlert {
                name: "MemoryPressure".to_string(),
                expr: r#"isolate_memory_bytes{type="current"} / isolate_memory_bytes{type="peak"} > 0.8"#.to_string(),
                duration: "10m".to_string(),
                severity: AlertSeverity::Warning,
                summary: "High memory pressure".to_string(),
                description: "Sandbox memory usage exceeds 80% of peak allocation".to_string(),
                labels: HashMap::from([("component".to_string(), "resource".to_string())]),
            },
            PrometheusAlert {
                name: "SandboxCreationFailures".to_string(),
                expr: r#"rate(isolate_sandbox_runs_total{status="failure"}[5m]) / rate(isolate_sandboxes_created_total[5m]) > 0.05"#.to_string(),
                duration: "5m".to_string(),
                severity: AlertSeverity::Critical,
                summary: "Sandbox creation failure rate too high".to_string(),
                description: "More than 5% of sandbox creations are failing".to_string(),
                labels: HashMap::from([("component".to_string(), "sandbox".to_string())]),
            },
            PrometheusAlert {
                name: "LowFuel".to_string(),
                expr: "deriv(isolate_fuel_consumed_total[10m]) > 0".to_string(),
                duration: "5m".to_string(),
                severity: AlertSeverity::Info,
                summary: "Fuel exhaustion rate increasing".to_string(),
                description: "Fuel consumption rate is trending upward, sandboxes may run out of fuel".to_string(),
                labels: HashMap::from([("component".to_string(), "resource".to_string())]),
            },
        ],
    }
}

/// Serialize a [`GrafanaDashboard`] to a JSON string.
pub fn dashboard_to_json(dashboard: &GrafanaDashboard) -> String {
    serde_json::to_string_pretty(dashboard).expect("dashboard serialization should not fail")
}

/// Serialize an [`AlertRuleGroup`] to a YAML-like format suitable for Prometheus
/// configuration. This avoids requiring a YAML crate dependency.
pub fn alerts_to_yaml(group: &AlertRuleGroup) -> String {
    let mut out = String::new();
    out.push_str("groups:\n");
    out.push_str(&format!("- name: {}\n", group.name));
    out.push_str(&format!("  interval: {}\n", group.interval));
    out.push_str("  rules:\n");

    for rule in &group.rules {
        out.push_str(&format!("  - alert: {}\n", rule.name));
        out.push_str(&format!("    expr: {}\n", rule.expr));
        out.push_str(&format!("    for: {}\n", rule.duration));
        out.push_str("    labels:\n");
        out.push_str(&format!("      severity: {}\n", rule.severity));
        for (k, v) in &rule.labels {
            out.push_str(&format!("      {}: {}\n", k, v));
        }
        out.push_str("    annotations:\n");
        out.push_str(&format!("      summary: {}\n", rule.summary));
        out.push_str(&format!("      description: {}\n", rule.description));
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dashboard_panel_count() {
        let dashboard = default_dashboard();
        assert_eq!(dashboard.panels.len(), 6);
    }

    #[test]
    fn test_default_dashboard_panels_have_valid_queries() {
        let dashboard = default_dashboard();
        for panel in &dashboard.panels {
            assert!(!panel.query.is_empty(), "panel '{}' has empty query", panel.title);
            assert!(
                panel.query.contains("isolate_"),
                "panel '{}' query should reference an isolate metric",
                panel.title
            );
        }
    }

    #[test]
    fn test_default_alerts_rule_count() {
        let alerts = default_alerts();
        assert_eq!(alerts.rules.len(), 5);
    }

    #[test]
    fn test_alert_severities() {
        let alerts = default_alerts();
        let severities: Vec<_> = alerts.rules.iter().map(|r| &r.severity).collect();
        assert_eq!(severities[0], &AlertSeverity::Critical); // HighErrorRate
        assert_eq!(severities[1], &AlertSeverity::Warning);  // HighLatency
        assert_eq!(severities[2], &AlertSeverity::Warning);  // MemoryPressure
        assert_eq!(severities[3], &AlertSeverity::Critical); // SandboxCreationFailures
        assert_eq!(severities[4], &AlertSeverity::Info);     // LowFuel
    }

    #[test]
    fn test_dashboard_to_json_valid() {
        let dashboard = default_dashboard();
        let json = dashboard_to_json(&dashboard);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should be valid JSON");
        assert_eq!(parsed["title"], "Isolate Sandbox Runtime");
        assert!(parsed["panels"].is_array());
    }

    #[test]
    fn test_alerts_to_yaml_format() {
        let alerts = default_alerts();
        let yaml = alerts_to_yaml(&alerts);
        assert!(yaml.starts_with("groups:\n"));
        assert!(yaml.contains("- name: isolate-alerts"));
        assert!(yaml.contains("interval: 30s"));
        assert!(yaml.contains("- alert: HighErrorRate"));
        assert!(yaml.contains("severity: critical"));
        assert!(yaml.contains("summary:"));
        assert!(yaml.contains("description:"));
    }

    #[test]
    fn test_panel_type_display() {
        assert_eq!(PanelType::Graph.to_string(), "graph");
        assert_eq!(PanelType::Stat.to_string(), "stat");
        assert_eq!(PanelType::Gauge.to_string(), "gauge");
        assert_eq!(PanelType::Table.to_string(), "table");
        assert_eq!(PanelType::Heatmap.to_string(), "heatmap");
    }

    #[test]
    fn test_alert_severity_display() {
        assert_eq!(AlertSeverity::Info.to_string(), "info");
        assert_eq!(AlertSeverity::Warning.to_string(), "warning");
        assert_eq!(AlertSeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn test_observability_config_default() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.scrape_interval, "15s");
        assert_eq!(config.metrics_path, "/metrics");
        assert_eq!(config.metrics_port, 9090);
        assert_eq!(config.dashboard.panels.len(), 6);
        assert_eq!(config.alerts.rules.len(), 5);
    }

    #[test]
    fn test_threshold_creation() {
        let t = Threshold {
            value: 42.0,
            color: "red".to_string(),
            label: Some("danger".to_string()),
        };
        assert_eq!(t.value, 42.0);
        assert_eq!(t.color, "red");
        assert_eq!(t.label, Some("danger".to_string()));
    }

    #[test]
    fn test_custom_dashboard() {
        let dashboard = GrafanaDashboard {
            title: "Custom".to_string(),
            description: "A custom dashboard".to_string(),
            tags: vec!["custom".to_string()],
            panels: vec![GrafanaPanel {
                id: 1,
                title: "Test Panel".to_string(),
                panel_type: PanelType::Table,
                query: "up".to_string(),
                description: None,
                unit: None,
                thresholds: vec![],
            }],
            refresh: "30s".to_string(),
            time_from: "now-6h".to_string(),
            time_to: "now".to_string(),
        };
        assert_eq!(dashboard.panels.len(), 1);
        assert_eq!(dashboard.title, "Custom");
    }

    #[test]
    fn test_custom_alert() {
        let alert = PrometheusAlert {
            name: "TestAlert".to_string(),
            expr: "up == 0".to_string(),
            duration: "1m".to_string(),
            severity: AlertSeverity::Critical,
            summary: "Target down".to_string(),
            description: "A monitored target is down".to_string(),
            labels: HashMap::from([("team".to_string(), "infra".to_string())]),
        };
        assert_eq!(alert.name, "TestAlert");
        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert!(alert.labels.contains_key("team"));
    }

    #[test]
    fn test_grafana_panel_all_fields() {
        let panel = GrafanaPanel {
            id: 99,
            title: "Full Panel".to_string(),
            panel_type: PanelType::Heatmap,
            query: "histogram_quantile(0.5, rate(isolate_sandbox_run_duration_seconds_bucket[5m]))".to_string(),
            description: Some("A heatmap panel with all fields populated".to_string()),
            unit: Some("s".to_string()),
            thresholds: vec![
                Threshold { value: 0.0, color: "green".to_string(), label: None },
                Threshold { value: 1.0, color: "orange".to_string(), label: Some("warn".to_string()) },
                Threshold { value: 5.0, color: "red".to_string(), label: Some("crit".to_string()) },
            ],
        };
        assert_eq!(panel.id, 99);
        assert_eq!(panel.panel_type, PanelType::Heatmap);
        assert_eq!(panel.thresholds.len(), 3);
        assert!(panel.description.is_some());
        assert_eq!(panel.unit, Some("s".to_string()));
    }
}
