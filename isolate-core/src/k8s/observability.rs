#![allow(dead_code)]
//! Prometheus metrics exporter and Grafana dashboard configuration for
//! production observability of the Isolate Kubernetes operator.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Metric types
// ---------------------------------------------------------------------------

/// The kind of a Prometheus metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

impl MetricType {
    fn as_str(&self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
        }
    }
}

/// Definition of a single Prometheus metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDefinition {
    /// Metric name (e.g. `isolate_sandbox_created_total`).
    pub name: String,
    /// HELP text.
    pub help: String,
    /// Metric type.
    pub metric_type: MetricType,
    /// Label names attached to every sample.
    pub labels: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal sample storage
// ---------------------------------------------------------------------------

/// A single labelled time-series.
#[derive(Debug, Clone)]
struct TimeSeries {
    label_values: Vec<String>,
    value: f64,
}

#[derive(Debug, Clone)]
struct HistogramSeries {
    label_values: Vec<String>,
    observations: Vec<f64>,
}

#[derive(Debug, Clone)]
struct RegisteredMetric {
    definition: MetricDefinition,
    series: Vec<TimeSeries>,
    histograms: Vec<HistogramSeries>,
}

// ---------------------------------------------------------------------------
// PrometheusExporter
// ---------------------------------------------------------------------------

/// Exports sandbox and operator metrics in Prometheus text exposition format.
#[derive(Debug, Clone)]
pub struct PrometheusExporter {
    metrics: Vec<RegisteredMetric>,
    /// Default histogram buckets.
    histogram_buckets: Vec<f64>,
}

impl PrometheusExporter {
    /// Create an exporter pre-loaded with the standard Isolate metrics.
    pub fn new() -> Self {
        let buckets = vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];

        let mut exporter = Self {
            metrics: Vec::new(),
            histogram_buckets: buckets,
        };

        // Pre-register standard metrics.
        exporter.register_metric(
            "isolate_sandbox_created_total",
            "Total number of sandboxes created",
            MetricType::Counter,
        );
        exporter.register_metric(
            "isolate_sandbox_terminated_total",
            "Total number of sandboxes terminated",
            MetricType::Counter,
        );
        exporter.register_metric(
            "isolate_sandbox_execution_duration_seconds",
            "Sandbox execution duration in seconds",
            MetricType::Histogram,
        );
        exporter.register_metric(
            "isolate_sandbox_memory_usage_bytes",
            "Current sandbox memory usage in bytes",
            MetricType::Gauge,
        );
        exporter.register_metric(
            "isolate_sandbox_fuel_consumed",
            "Fuel consumed per sandbox execution",
            MetricType::Histogram,
        );

        exporter
    }

    /// Register a custom metric. If a metric with the same name already exists
    /// it is silently ignored.
    pub fn register_metric(&mut self, name: &str, help: &str, metric_type: MetricType) {
        if self.metrics.iter().any(|m| m.definition.name == name) {
            return;
        }
        let labels = match metric_type {
            MetricType::Counter | MetricType::Gauge => vec!["tenant".to_string(), "namespace".to_string()],
            MetricType::Histogram => vec!["tenant".to_string()],
        };
        self.metrics.push(RegisteredMetric {
            definition: MetricDefinition {
                name: name.to_string(),
                help: help.to_string(),
                metric_type,
                labels,
            },
            series: Vec::new(),
            histograms: Vec::new(),
        });
    }

    // -- convenience recording helpers --------------------------------------

    /// Increment the `isolate_sandbox_created_total` counter.
    pub fn record_sandbox_created(&mut self, tenant: &str, namespace: &str) {
        self.inc_counter("isolate_sandbox_created_total", &[tenant, namespace]);
    }

    /// Increment the `isolate_sandbox_terminated_total` counter.
    pub fn record_sandbox_terminated(&mut self, tenant: &str, namespace: &str, exit_code: i32) {
        // Encode exit code in the namespace label for simplicity.
        let ns_label = format!("{namespace}:exit_{exit_code}");
        self.inc_counter("isolate_sandbox_terminated_total", &[tenant, &ns_label]);
    }

    /// Observe `isolate_sandbox_execution_duration_seconds`.
    pub fn record_execution_duration(&mut self, tenant: &str, duration: Duration) {
        self.observe_histogram(
            "isolate_sandbox_execution_duration_seconds",
            &[tenant],
            duration.as_secs_f64(),
        );
    }

    /// Set `isolate_sandbox_memory_usage_bytes` gauge.
    pub fn record_memory_usage(&mut self, tenant: &str, bytes: u64) {
        self.set_gauge(
            "isolate_sandbox_memory_usage_bytes",
            &["tenant", tenant],
            bytes as f64,
        );
    }

    /// Observe `isolate_sandbox_fuel_consumed` histogram.
    pub fn record_fuel_consumed(&mut self, tenant: &str, fuel: u64) {
        self.observe_histogram("isolate_sandbox_fuel_consumed", &[tenant], fuel as f64);
    }

    // -- low-level helpers --------------------------------------------------

    fn inc_counter(&mut self, name: &str, label_values: &[&str]) {
        if let Some(m) = self.metrics.iter_mut().find(|m| m.definition.name == name) {
            let lv: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();
            if let Some(ts) = m.series.iter_mut().find(|ts| ts.label_values == lv) {
                ts.value += 1.0;
            } else {
                m.series.push(TimeSeries {
                    label_values: lv,
                    value: 1.0,
                });
            }
        }
    }

    fn set_gauge(&mut self, name: &str, label_values: &[&str], value: f64) {
        if let Some(m) = self.metrics.iter_mut().find(|m| m.definition.name == name) {
            let lv: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();
            if let Some(ts) = m.series.iter_mut().find(|ts| ts.label_values == lv) {
                ts.value = value;
            } else {
                m.series.push(TimeSeries {
                    label_values: lv,
                    value,
                });
            }
        }
    }

    fn observe_histogram(&mut self, name: &str, label_values: &[&str], value: f64) {
        if let Some(m) = self.metrics.iter_mut().find(|m| m.definition.name == name) {
            let lv: Vec<String> = label_values.iter().map(|s| s.to_string()).collect();
            if let Some(hs) = m.histograms.iter_mut().find(|h| h.label_values == lv) {
                hs.observations.push(value);
            } else {
                m.histograms.push(HistogramSeries {
                    label_values: lv,
                    observations: vec![value],
                });
            }
        }
    }

    /// Render all registered metrics in Prometheus text exposition format.
    pub fn render(&self) -> String {
        let mut out = String::new();

        for m in &self.metrics {
            out.push_str(&format!("# HELP {} {}\n", m.definition.name, m.definition.help));
            out.push_str(&format!(
                "# TYPE {} {}\n",
                m.definition.name,
                m.definition.metric_type.as_str()
            ));

            // Counter / Gauge series
            for ts in &m.series {
                let labels = Self::format_labels(&m.definition.labels, &ts.label_values);
                out.push_str(&format!("{}{} {}\n", m.definition.name, labels, ts.value));
            }

            // Histogram series
            for hs in &m.histograms {
                let label_str = Self::format_labels(&m.definition.labels, &hs.label_values);
                let count = hs.observations.len();
                let sum: f64 = hs.observations.iter().sum();

                for bucket in &self.histogram_buckets {
                    let le_count = hs.observations.iter().filter(|&&v| v <= *bucket).count();
                    out.push_str(&format!(
                        "{}_bucket{{{},le=\"{}\"}} {}\n",
                        m.definition.name,
                        Self::labels_inner(&m.definition.labels, &hs.label_values),
                        bucket,
                        le_count,
                    ));
                }
                // +Inf bucket
                out.push_str(&format!(
                    "{}_bucket{{{},le=\"+Inf\"}} {}\n",
                    m.definition.name,
                    Self::labels_inner(&m.definition.labels, &hs.label_values),
                    count,
                ));
                out.push_str(&format!(
                    "{}_sum{} {}\n",
                    m.definition.name, label_str, sum
                ));
                out.push_str(&format!(
                    "{}_count{} {}\n",
                    m.definition.name, label_str, count
                ));
            }
        }

        out
    }

    fn format_labels(names: &[String], values: &[String]) -> String {
        let inner = Self::labels_inner(names, values);
        if inner.is_empty() {
            String::new()
        } else {
            format!("{{{inner}}}")
        }
    }

    fn labels_inner(names: &[String], values: &[String]) -> String {
        names
            .iter()
            .zip(values.iter())
            .map(|(n, v)| format!("{n}=\"{v}\""))
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PrometheusOperatorMetrics — Prometheus-style metrics for the K8s operator
// (named to avoid conflict with `operator::OperatorMetrics`)
// ---------------------------------------------------------------------------

/// Prometheus metric definitions specific to the Kubernetes operator
/// reconciliation loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusOperatorMetrics {
    /// Total reconciliation attempts (counter).
    pub reconciliation_total: u64,
    /// Total reconciliation errors (counter).
    pub reconciliation_errors: u64,
    /// Reconciliation duration in seconds (histogram observations).
    pub reconciliation_duration_seconds: Vec<f64>,
    /// Active sandboxes keyed by namespace (gauge).
    pub active_sandboxes: HashMap<String, u64>,
    /// Number of sandboxes in Pending phase (gauge).
    pub pending_sandboxes: u64,
    /// Sandbox creation latency observations in seconds.
    pub sandbox_creation_latency: Vec<f64>,
    /// Pool utilisation ratio 0.0–1.0 (gauge).
    pub pool_utilization: f64,
}

impl PrometheusOperatorMetrics {
    /// Create a zero-valued instance.
    pub fn new() -> Self {
        Self {
            reconciliation_total: 0,
            reconciliation_errors: 0,
            reconciliation_duration_seconds: Vec::new(),
            active_sandboxes: HashMap::new(),
            pending_sandboxes: 0,
            sandbox_creation_latency: Vec::new(),
            pool_utilization: 0.0,
        }
    }

    /// Record a successful reconciliation.
    pub fn record_reconciliation(&mut self, duration: Duration) {
        self.reconciliation_total += 1;
        self.reconciliation_duration_seconds.push(duration.as_secs_f64());
    }

    /// Record a reconciliation error.
    pub fn record_reconciliation_error(&mut self, duration: Duration) {
        self.reconciliation_total += 1;
        self.reconciliation_errors += 1;
        self.reconciliation_duration_seconds.push(duration.as_secs_f64());
    }

    /// Set the active sandbox count for a namespace.
    pub fn set_active_sandboxes(&mut self, namespace: &str, count: u64) {
        self.active_sandboxes.insert(namespace.to_string(), count);
    }

    /// Record sandbox creation latency.
    pub fn record_creation_latency(&mut self, latency: Duration) {
        self.sandbox_creation_latency.push(latency.as_secs_f64());
    }

    /// Set current pool utilisation (0.0–1.0).
    pub fn set_pool_utilization(&mut self, utilization: f64) {
        self.pool_utilization = utilization.clamp(0.0, 1.0);
    }

    /// Render these operator metrics in Prometheus text exposition format.
    pub fn render(&self) -> String {
        let mut out = String::new();

        out.push_str("# HELP isolate_operator_reconciliation_total Total reconciliation attempts\n");
        out.push_str("# TYPE isolate_operator_reconciliation_total counter\n");
        out.push_str(&format!(
            "isolate_operator_reconciliation_total {}\n",
            self.reconciliation_total
        ));

        out.push_str(
            "# HELP isolate_operator_reconciliation_errors_total Total reconciliation errors\n",
        );
        out.push_str("# TYPE isolate_operator_reconciliation_errors_total counter\n");
        out.push_str(&format!(
            "isolate_operator_reconciliation_errors_total {}\n",
            self.reconciliation_errors
        ));

        out.push_str(
            "# HELP isolate_operator_active_sandboxes Active sandboxes by namespace\n",
        );
        out.push_str("# TYPE isolate_operator_active_sandboxes gauge\n");
        for (ns, count) in &self.active_sandboxes {
            out.push_str(&format!(
                "isolate_operator_active_sandboxes{{namespace=\"{ns}\"}} {count}\n"
            ));
        }

        out.push_str("# HELP isolate_operator_pending_sandboxes Pending sandboxes\n");
        out.push_str("# TYPE isolate_operator_pending_sandboxes gauge\n");
        out.push_str(&format!(
            "isolate_operator_pending_sandboxes {}\n",
            self.pending_sandboxes
        ));

        out.push_str("# HELP isolate_operator_pool_utilization Pool utilization ratio\n");
        out.push_str("# TYPE isolate_operator_pool_utilization gauge\n");
        out.push_str(&format!(
            "isolate_operator_pool_utilization {}\n",
            self.pool_utilization
        ));

        out
    }
}

impl Default for PrometheusOperatorMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Grafana dashboard generation
// ---------------------------------------------------------------------------

/// Generates Grafana dashboard JSON for Isolate observability.
pub struct GrafanaDashboard;

impl GrafanaDashboard {
    /// Build the "Sandbox Overview" dashboard.
    pub fn sandbox_overview_dashboard() -> serde_json::Value {
        serde_json::json!({
            "dashboard": {
                "id": null,
                "uid": "isolate-sandbox-overview",
                "title": "Isolate Sandbox Overview",
                "tags": ["isolate", "sandbox", "wasm"],
                "timezone": "browser",
                "schemaVersion": 39,
                "version": 1,
                "refresh": "10s",
                "time": {
                    "from": "now-1h",
                    "to": "now"
                },
                "panels": [
                    Self::panel_sandbox_count(),
                    Self::panel_creation_latency(),
                    Self::panel_memory_usage(),
                    Self::panel_fuel_consumption(),
                    Self::panel_error_rate(),
                ]
            }
        })
    }

    fn panel_sandbox_count() -> serde_json::Value {
        serde_json::json!({
            "id": 1,
            "title": "Sandbox Count",
            "type": "stat",
            "gridPos": { "h": 8, "w": 6, "x": 0, "y": 0 },
            "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
            "targets": [{
                "expr": "sum(isolate_operator_active_sandboxes)",
                "legendFormat": "Active",
                "refId": "A"
            }],
            "fieldConfig": {
                "defaults": {
                    "thresholds": {
                        "steps": [
                            { "color": "green", "value": null },
                            { "color": "yellow", "value": 50 },
                            { "color": "red", "value": 100 }
                        ]
                    }
                }
            }
        })
    }

    fn panel_creation_latency() -> serde_json::Value {
        serde_json::json!({
            "id": 2,
            "title": "Sandbox Creation Latency",
            "type": "timeseries",
            "gridPos": { "h": 8, "w": 6, "x": 6, "y": 0 },
            "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
            "targets": [
                {
                    "expr": "histogram_quantile(0.50, rate(isolate_sandbox_execution_duration_seconds_bucket[5m]))",
                    "legendFormat": "p50",
                    "refId": "A"
                },
                {
                    "expr": "histogram_quantile(0.95, rate(isolate_sandbox_execution_duration_seconds_bucket[5m]))",
                    "legendFormat": "p95",
                    "refId": "B"
                },
                {
                    "expr": "histogram_quantile(0.99, rate(isolate_sandbox_execution_duration_seconds_bucket[5m]))",
                    "legendFormat": "p99",
                    "refId": "C"
                }
            ],
            "fieldConfig": {
                "defaults": {
                    "unit": "s"
                }
            }
        })
    }

    fn panel_memory_usage() -> serde_json::Value {
        serde_json::json!({
            "id": 3,
            "title": "Memory Usage",
            "type": "timeseries",
            "gridPos": { "h": 8, "w": 6, "x": 12, "y": 0 },
            "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
            "targets": [{
                "expr": "isolate_sandbox_memory_usage_bytes",
                "legendFormat": "{{tenant}}",
                "refId": "A"
            }],
            "fieldConfig": {
                "defaults": {
                    "unit": "bytes"
                }
            }
        })
    }

    fn panel_fuel_consumption() -> serde_json::Value {
        serde_json::json!({
            "id": 4,
            "title": "Fuel Consumption",
            "type": "timeseries",
            "gridPos": { "h": 8, "w": 6, "x": 18, "y": 0 },
            "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
            "targets": [{
                "expr": "rate(isolate_sandbox_fuel_consumed_sum[5m])",
                "legendFormat": "{{tenant}}",
                "refId": "A"
            }],
            "fieldConfig": {
                "defaults": {
                    "unit": "short"
                }
            }
        })
    }

    fn panel_error_rate() -> serde_json::Value {
        serde_json::json!({
            "id": 5,
            "title": "Error Rate",
            "type": "timeseries",
            "gridPos": { "h": 8, "w": 12, "x": 0, "y": 8 },
            "datasource": { "type": "prometheus", "uid": "${DS_PROMETHEUS}" },
            "targets": [{
                "expr": "rate(isolate_operator_reconciliation_errors_total[5m]) / rate(isolate_operator_reconciliation_total[5m])",
                "legendFormat": "error ratio",
                "refId": "A"
            }],
            "fieldConfig": {
                "defaults": {
                    "unit": "percentunit",
                    "min": 0,
                    "max": 1,
                    "thresholds": {
                        "steps": [
                            { "color": "green", "value": null },
                            { "color": "yellow", "value": 0.01 },
                            { "color": "red", "value": 0.05 }
                        ]
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Alert rules
// ---------------------------------------------------------------------------

/// A single Prometheus alerting rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// Alert name.
    pub name: String,
    /// PromQL expression.
    pub expression: String,
    /// Duration the expression must hold before firing.
    pub for_duration: Duration,
    /// Severity label (e.g. `critical`, `warning`).
    pub severity: String,
    /// Human-readable summary.
    pub summary: String,
}

/// A collection of Prometheus alert rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleSet {
    /// Contained rules.
    pub rules: Vec<AlertRule>,
}

impl AlertRuleSet {
    /// Pre-built rules for common production conditions.
    pub fn default_rules() -> Self {
        Self {
            rules: vec![
                AlertRule {
                    name: "IsolateHighErrorRate".to_string(),
                    expression:
                        "rate(isolate_operator_reconciliation_errors_total[5m]) / rate(isolate_operator_reconciliation_total[5m]) > 0.05"
                            .to_string(),
                    for_duration: Duration::from_secs(300),
                    severity: "critical".to_string(),
                    summary: "Isolate operator error rate exceeds 5% for 5 minutes".to_string(),
                },
                AlertRule {
                    name: "IsolateHighMemoryUsage".to_string(),
                    expression:
                        "isolate_sandbox_memory_usage_bytes / isolate_sandbox_memory_limit_bytes > 0.9"
                            .to_string(),
                    for_duration: Duration::from_secs(600),
                    severity: "warning".to_string(),
                    summary: "Sandbox memory usage above 90% for 10 minutes".to_string(),
                },
                AlertRule {
                    name: "IsolatePoolExhaustion".to_string(),
                    expression: "isolate_operator_pool_utilization > 0.95".to_string(),
                    for_duration: Duration::from_secs(300),
                    severity: "critical".to_string(),
                    summary: "Sandbox pool utilization above 95%".to_string(),
                },
                AlertRule {
                    name: "IsolateSlowSandboxCreation".to_string(),
                    expression:
                        "histogram_quantile(0.99, rate(isolate_sandbox_execution_duration_seconds_bucket[5m])) > 0.1"
                            .to_string(),
                    for_duration: Duration::from_secs(300),
                    severity: "warning".to_string(),
                    summary: "Sandbox creation p99 latency exceeds 100ms for 5 minutes"
                        .to_string(),
                },
            ],
        }
    }

    /// Render the rule set as a Prometheus alerting rules YAML document.
    pub fn to_yaml(&self) -> String {
        let mut out = String::from("groups:\n  - name: isolate.rules\n    rules:\n");

        for rule in &self.rules {
            let for_secs = rule.for_duration.as_secs();
            let for_str = if for_secs >= 60 {
                format!("{}m", for_secs / 60)
            } else {
                format!("{for_secs}s")
            };

            out.push_str(&format!("      - alert: {}\n", rule.name));
            out.push_str(&format!("        expr: {}\n", rule.expression));
            out.push_str(&format!("        for: {}\n", for_str));
            out.push_str("        labels:\n");
            out.push_str(&format!("          severity: {}\n", rule.severity));
            out.push_str("        annotations:\n");
            out.push_str(&format!("          summary: {}\n", rule.summary));
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- PrometheusExporter -------------------------------------------------

    #[test]
    fn test_exporter_new_registers_standard_metrics() {
        let exporter = PrometheusExporter::new();
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_created_total"));
        assert!(rendered.contains("isolate_sandbox_terminated_total"));
        assert!(rendered.contains("isolate_sandbox_execution_duration_seconds"));
        assert!(rendered.contains("isolate_sandbox_memory_usage_bytes"));
        assert!(rendered.contains("isolate_sandbox_fuel_consumed"));
    }

    #[test]
    fn test_exporter_register_custom_metric() {
        let mut exporter = PrometheusExporter::new();
        exporter.register_metric("my_custom_counter", "A custom counter", MetricType::Counter);
        let rendered = exporter.render();
        assert!(rendered.contains("my_custom_counter"));
        assert!(rendered.contains("A custom counter"));
    }

    #[test]
    fn test_exporter_duplicate_registration_ignored() {
        let mut exporter = PrometheusExporter::new();
        let before = exporter.metrics.len();
        exporter.register_metric(
            "isolate_sandbox_created_total",
            "duplicate",
            MetricType::Counter,
        );
        assert_eq!(exporter.metrics.len(), before);
    }

    #[test]
    fn test_record_sandbox_created() {
        let mut exporter = PrometheusExporter::new();
        exporter.record_sandbox_created("acme", "default");
        exporter.record_sandbox_created("acme", "default");
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_created_total{tenant=\"acme\",namespace=\"default\"} 2"));
    }

    #[test]
    fn test_record_sandbox_terminated() {
        let mut exporter = PrometheusExporter::new();
        exporter.record_sandbox_terminated("acme", "default", 0);
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_terminated_total"));
        assert!(rendered.contains("exit_0"));
    }

    #[test]
    fn test_record_execution_duration() {
        let mut exporter = PrometheusExporter::new();
        exporter.record_execution_duration("acme", Duration::from_millis(50));
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_execution_duration_seconds_bucket"));
        assert!(rendered.contains("isolate_sandbox_execution_duration_seconds_count"));
        assert!(rendered.contains("isolate_sandbox_execution_duration_seconds_sum"));
    }

    #[test]
    fn test_record_memory_usage_gauge() {
        let mut exporter = PrometheusExporter::new();
        exporter.record_memory_usage("acme", 1024 * 1024);
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_memory_usage_bytes"));
        assert!(rendered.contains("1048576"));
    }

    #[test]
    fn test_record_fuel_consumed() {
        let mut exporter = PrometheusExporter::new();
        exporter.record_fuel_consumed("acme", 500_000);
        let rendered = exporter.render();
        assert!(rendered.contains("isolate_sandbox_fuel_consumed_bucket"));
        assert!(rendered.contains("500000"));
    }

    #[test]
    fn test_render_contains_help_and_type() {
        let exporter = PrometheusExporter::new();
        let rendered = exporter.render();
        assert!(rendered.contains("# HELP"));
        assert!(rendered.contains("# TYPE"));
    }

    // -- PrometheusOperatorMetrics ------------------------------------------

    #[test]
    fn test_operator_metrics_record_reconciliation() {
        let mut m = PrometheusOperatorMetrics::new();
        m.record_reconciliation(Duration::from_millis(12));
        assert_eq!(m.reconciliation_total, 1);
        assert_eq!(m.reconciliation_errors, 0);
        assert_eq!(m.reconciliation_duration_seconds.len(), 1);
    }

    #[test]
    fn test_operator_metrics_record_error() {
        let mut m = PrometheusOperatorMetrics::new();
        m.record_reconciliation_error(Duration::from_millis(5));
        assert_eq!(m.reconciliation_total, 1);
        assert_eq!(m.reconciliation_errors, 1);
    }

    #[test]
    fn test_operator_metrics_render() {
        let mut m = PrometheusOperatorMetrics::new();
        m.set_active_sandboxes("default", 5);
        m.pending_sandboxes = 2;
        m.set_pool_utilization(0.75);
        let rendered = m.render();
        assert!(rendered.contains("isolate_operator_active_sandboxes{namespace=\"default\"} 5"));
        assert!(rendered.contains("isolate_operator_pending_sandboxes 2"));
        assert!(rendered.contains("isolate_operator_pool_utilization 0.75"));
    }

    #[test]
    fn test_pool_utilization_clamped() {
        let mut m = PrometheusOperatorMetrics::new();
        m.set_pool_utilization(1.5);
        assert!((m.pool_utilization - 1.0).abs() < f64::EPSILON);
        m.set_pool_utilization(-0.5);
        assert!(m.pool_utilization.abs() < f64::EPSILON);
    }

    // -- GrafanaDashboard ---------------------------------------------------

    #[test]
    fn test_grafana_dashboard_structure() {
        let dash = GrafanaDashboard::sandbox_overview_dashboard();
        let db = &dash["dashboard"];
        assert_eq!(db["uid"], "isolate-sandbox-overview");
        assert_eq!(db["title"], "Isolate Sandbox Overview");
        let panels = db["panels"].as_array().unwrap();
        assert_eq!(panels.len(), 5);
    }

    #[test]
    fn test_grafana_dashboard_panel_titles() {
        let dash = GrafanaDashboard::sandbox_overview_dashboard();
        let panels = dash["dashboard"]["panels"].as_array().unwrap();
        let titles: Vec<&str> = panels.iter().map(|p| p["title"].as_str().unwrap()).collect();
        assert!(titles.contains(&"Sandbox Count"));
        assert!(titles.contains(&"Sandbox Creation Latency"));
        assert!(titles.contains(&"Memory Usage"));
        assert!(titles.contains(&"Fuel Consumption"));
        assert!(titles.contains(&"Error Rate"));
    }

    // -- AlertRuleSet -------------------------------------------------------

    #[test]
    fn test_default_alert_rules_count() {
        let rules = AlertRuleSet::default_rules();
        assert_eq!(rules.rules.len(), 4);
    }

    #[test]
    fn test_alert_rules_to_yaml() {
        let rules = AlertRuleSet::default_rules();
        let yaml = rules.to_yaml();
        assert!(yaml.contains("groups:"));
        assert!(yaml.contains("isolate.rules"));
        assert!(yaml.contains("IsolateHighErrorRate"));
        assert!(yaml.contains("IsolateHighMemoryUsage"));
        assert!(yaml.contains("IsolatePoolExhaustion"));
        assert!(yaml.contains("IsolateSlowSandboxCreation"));
        assert!(yaml.contains("severity: critical"));
        assert!(yaml.contains("severity: warning"));
        assert!(yaml.contains("for: 5m"));
        assert!(yaml.contains("for: 10m"));
    }

    #[test]
    fn test_alert_rule_serialization() {
        let rule = AlertRule {
            name: "TestAlert".to_string(),
            expression: "up == 0".to_string(),
            for_duration: Duration::from_secs(60),
            severity: "critical".to_string(),
            summary: "Target is down".to_string(),
        };
        let json = serde_json::to_string(&rule).unwrap();
        assert!(json.contains("TestAlert"));
        let deserialized: AlertRule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "TestAlert");
    }

    #[test]
    fn test_metric_definition_serialization() {
        let def = MetricDefinition {
            name: "test_metric".to_string(),
            help: "A test".to_string(),
            metric_type: MetricType::Counter,
            labels: vec!["env".to_string()],
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: MetricDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test_metric");
        assert_eq!(back.metric_type, MetricType::Counter);
    }
}
