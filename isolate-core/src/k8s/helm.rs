//! Helm chart generation for Isolate operator deployment.
//!
//! This module provides utilities to generate Helm chart files
//! for deploying the Isolate operator and CRDs to Kubernetes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Helm chart metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartMetadata {
    /// Chart API version.
    pub api_version: String,
    /// Chart name.
    pub name: String,
    /// Chart version.
    pub version: String,
    /// Kubernetes version constraint.
    pub kube_version: Option<String>,
    /// Chart description.
    pub description: String,
    /// Chart type (application or library).
    #[serde(rename = "type")]
    pub chart_type: String,
    /// Keywords for discovery.
    pub keywords: Vec<String>,
    /// Home URL.
    pub home: Option<String>,
    /// Source URLs.
    pub sources: Vec<String>,
    /// Maintainers.
    pub maintainers: Vec<Maintainer>,
    /// Application version.
    pub app_version: String,
    /// Deprecated flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    /// Icon URL.
    pub icon: Option<String>,
}

impl Default for ChartMetadata {
    fn default() -> Self {
        Self {
            api_version: "v2".to_string(),
            name: "isolate-operator".to_string(),
            version: "0.1.0".to_string(),
            kube_version: Some(">= 1.27.0".to_string()),
            description: "Isolate - Secure WASM Sandbox Runtime Operator".to_string(),
            chart_type: "application".to_string(),
            keywords: vec![
                "wasm".to_string(),
                "sandbox".to_string(),
                "isolate".to_string(),
                "security".to_string(),
                "kubernetes".to_string(),
                "operator".to_string(),
            ],
            home: Some("https://github.com/isolate-runtime/isolate".to_string()),
            sources: vec!["https://github.com/isolate-runtime/isolate".to_string()],
            maintainers: vec![Maintainer {
                name: "Isolate Team".to_string(),
                email: Some("team@isolate.dev".to_string()),
                url: None,
            }],
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            deprecated: None,
            icon: None,
        }
    }
}

/// Chart maintainer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Maintainer {
    /// Maintainer name.
    pub name: String,
    /// Maintainer email.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Maintainer URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Helm values configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmValues {
    /// Number of operator replicas.
    pub replica_count: u32,
    /// Image configuration.
    pub image: ImageConfig,
    /// Image pull secrets.
    pub image_pull_secrets: Vec<String>,
    /// Service account configuration.
    pub service_account: ServiceAccountConfig,
    /// Pod annotations.
    pub pod_annotations: HashMap<String, String>,
    /// Pod security context.
    pub pod_security_context: SecurityContext,
    /// Container security context.
    pub security_context: SecurityContext,
    /// Service configuration.
    pub service: ServiceConfig,
    /// Resource limits.
    pub resources: ResourceConfig,
    /// Node selector.
    pub node_selector: HashMap<String, String>,
    /// Tolerations.
    pub tolerations: Vec<TolerationConfig>,
    /// Affinity.
    pub affinity: HashMap<String, serde_json::Value>,
    /// Operator configuration.
    pub operator: OperatorValues,
    /// Metrics configuration.
    pub metrics: MetricsConfig,
    /// RBAC configuration.
    pub rbac: RbacConfig,
}

impl Default for HelmValues {
    fn default() -> Self {
        Self {
            replica_count: 1,
            image: ImageConfig::default(),
            image_pull_secrets: vec![],
            service_account: ServiceAccountConfig::default(),
            pod_annotations: HashMap::new(),
            pod_security_context: SecurityContext {
                run_as_non_root: Some(true),
                fs_group: Some(1000),
                ..Default::default()
            },
            security_context: SecurityContext {
                run_as_non_root: Some(true),
                run_as_user: Some(1000),
                allow_privilege_escalation: Some(false),
                read_only_root_filesystem: Some(true),
                ..Default::default()
            },
            service: ServiceConfig::default(),
            resources: ResourceConfig::default(),
            node_selector: HashMap::new(),
            tolerations: vec![],
            affinity: HashMap::new(),
            operator: OperatorValues::default(),
            metrics: MetricsConfig::default(),
            rbac: RbacConfig::default(),
        }
    }
}

/// Image configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    /// Image repository.
    pub repository: String,
    /// Image pull policy.
    pub pull_policy: String,
    /// Image tag (defaults to chart appVersion).
    pub tag: String,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            repository: "ghcr.io/isolate-runtime/isolate-operator".to_string(),
            pull_policy: "IfNotPresent".to_string(),
            tag: "".to_string(),
        }
    }
}

/// Service account configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountConfig {
    /// Create service account.
    pub create: bool,
    /// Service account annotations.
    pub annotations: HashMap<String, String>,
    /// Service account name (auto-generated if empty).
    pub name: String,
}

impl Default for ServiceAccountConfig {
    fn default() -> Self {
        Self {
            create: true,
            annotations: HashMap::new(),
            name: String::new(),
        }
    }
}

/// Security context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_as_non_root: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_as_user: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_as_group: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_group: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_privilege_escalation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_root_filesystem: Option<bool>,
}

/// Service configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    /// Service type.
    #[serde(rename = "type")]
    pub service_type: String,
    /// Service port.
    pub port: u16,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            service_type: "ClusterIP".to_string(),
            port: 8080,
        }
    }
}

/// Resource configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Resource limits.
    pub limits: ResourceLimits,
    /// Resource requests.
    pub requests: ResourceLimits,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            limits: ResourceLimits {
                cpu: "500m".to_string(),
                memory: "512Mi".to_string(),
            },
            requests: ResourceLimits {
                cpu: "100m".to_string(),
                memory: "128Mi".to_string(),
            },
        }
    }
}

/// Resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// CPU limit.
    pub cpu: String,
    /// Memory limit.
    pub memory: String,
}

/// Toleration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TolerationConfig {
    /// Toleration key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Toleration operator.
    pub operator: String,
    /// Toleration value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Toleration effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
}

/// Operator-specific values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorValues {
    /// Namespaces to watch (empty = all).
    pub watch_namespaces: Vec<String>,
    /// Enable leader election.
    pub leader_election: bool,
    /// Log level.
    pub log_level: String,
    /// Reconcile interval in seconds.
    pub reconcile_interval_seconds: u32,
    /// Maximum concurrent reconciles.
    pub max_concurrent_reconciles: u32,
    /// Scheduling strategy.
    pub scheduling_strategy: String,
}

impl Default for OperatorValues {
    fn default() -> Self {
        Self {
            watch_namespaces: vec![],
            leader_election: true,
            log_level: "info".to_string(),
            reconcile_interval_seconds: 30,
            max_concurrent_reconciles: 10,
            scheduling_strategy: "LeastLoaded".to_string(),
        }
    }
}

/// Metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsConfig {
    /// Enable metrics.
    pub enabled: bool,
    /// Metrics port.
    pub port: u16,
    /// Create ServiceMonitor.
    pub service_monitor: ServiceMonitorConfig,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 8081,
            service_monitor: ServiceMonitorConfig::default(),
        }
    }
}

/// ServiceMonitor configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceMonitorConfig {
    /// Create ServiceMonitor.
    pub enabled: bool,
    /// Scrape interval.
    pub interval: String,
    /// Additional labels.
    pub additional_labels: HashMap<String, String>,
}

impl Default for ServiceMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: "30s".to_string(),
            additional_labels: HashMap::new(),
        }
    }
}

/// RBAC configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RbacConfig {
    /// Create RBAC resources.
    pub create: bool,
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self { create: true }
    }
}

/// Helm chart generator.
pub struct HelmChartGenerator {
    metadata: ChartMetadata,
    values: HelmValues,
}

impl HelmChartGenerator {
    /// Create a new generator with default configuration.
    pub fn new() -> Self {
        Self {
            metadata: ChartMetadata::default(),
            values: HelmValues::default(),
        }
    }

    /// Set chart metadata.
    pub fn with_metadata(mut self, metadata: ChartMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set values.
    pub fn with_values(mut self, values: HelmValues) -> Self {
        self.values = values;
        self
    }

    /// Generate Chart.yaml content.
    pub fn generate_chart_yaml(&self) -> String {
        serde_yaml::to_string(&self.metadata).unwrap_or_else(|_| "# Error generating Chart.yaml".to_string())
    }

    /// Generate values.yaml content.
    pub fn generate_values_yaml(&self) -> String {
        serde_yaml::to_string(&self.values).unwrap_or_else(|_| "# Error generating values.yaml".to_string())
    }

    /// Generate the deployment template.
    pub fn generate_deployment_template(&self) -> String {
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "isolate-operator.fullname" . }}
  labels:
    {{- include "isolate-operator.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.replicaCount }}
  selector:
    matchLabels:
      {{- include "isolate-operator.selectorLabels" . | nindent 6 }}
  template:
    metadata:
      {{- with .Values.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      labels:
        {{- include "isolate-operator.selectorLabels" . | nindent 8 }}
    spec:
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      serviceAccountName: {{ include "isolate-operator.serviceAccountName" . }}
      securityContext:
        {{- toYaml .Values.podSecurityContext | nindent 8 }}
      containers:
        - name: {{ .Chart.Name }}
          securityContext:
            {{- toYaml .Values.securityContext | nindent 12 }}
          image: "{{ .Values.image.repository }}:{{ .Values.image.tag | default .Chart.AppVersion }}"
          imagePullPolicy: {{ .Values.image.pullPolicy }}
          args:
            - --leader-election={{ .Values.operator.leaderElection }}
            - --log-level={{ .Values.operator.logLevel }}
            - --reconcile-interval={{ .Values.operator.reconcileIntervalSeconds }}s
            - --max-concurrent-reconciles={{ .Values.operator.maxConcurrentReconciles }}
            - --scheduling-strategy={{ .Values.operator.schedulingStrategy }}
            {{- range .Values.operator.watchNamespaces }}
            - --watch-namespace={{ . }}
            {{- end }}
          ports:
            - name: http
              containerPort: {{ .Values.service.port }}
              protocol: TCP
            {{- if .Values.metrics.enabled }}
            - name: metrics
              containerPort: {{ .Values.metrics.port }}
              protocol: TCP
            {{- end }}
          livenessProbe:
            httpGet:
              path: /healthz
              port: http
            initialDelaySeconds: 15
            periodSeconds: 20
          readinessProbe:
            httpGet:
              path: /readyz
              port: http
            initialDelaySeconds: 5
            periodSeconds: 10
          resources:
            {{- toYaml .Values.resources | nindent 12 }}
      {{- with .Values.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
"#
        .to_string()
    }

    /// Generate the service template.
    pub fn generate_service_template(&self) -> String {
        r#"apiVersion: v1
kind: Service
metadata:
  name: {{ include "isolate-operator.fullname" . }}
  labels:
    {{- include "isolate-operator.labels" . | nindent 4 }}
spec:
  type: {{ .Values.service.type }}
  ports:
    - port: {{ .Values.service.port }}
      targetPort: http
      protocol: TCP
      name: http
    {{- if .Values.metrics.enabled }}
    - port: {{ .Values.metrics.port }}
      targetPort: metrics
      protocol: TCP
      name: metrics
    {{- end }}
  selector:
    {{- include "isolate-operator.selectorLabels" . | nindent 4 }}
"#
        .to_string()
    }

    /// Generate the RBAC templates.
    pub fn generate_rbac_templates(&self) -> String {
        r#"{{- if .Values.rbac.create -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "isolate-operator.fullname" . }}
  labels:
    {{- include "isolate-operator.labels" . | nindent 4 }}
rules:
  - apiGroups:
      - isolate.io
    resources:
      - sandboxes
      - sandboxes/status
      - sandboxpools
      - sandboxpools/status
    verbs:
      - create
      - delete
      - get
      - list
      - patch
      - update
      - watch
  - apiGroups:
      - ""
    resources:
      - pods
      - pods/status
      - services
      - configmaps
      - secrets
      - events
    verbs:
      - create
      - delete
      - get
      - list
      - patch
      - update
      - watch
  - apiGroups:
      - ""
    resources:
      - nodes
    verbs:
      - get
      - list
      - watch
  - apiGroups:
      - coordination.k8s.io
    resources:
      - leases
    verbs:
      - create
      - delete
      - get
      - list
      - patch
      - update
      - watch
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "isolate-operator.fullname" . }}
  labels:
    {{- include "isolate-operator.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "isolate-operator.fullname" . }}
subjects:
  - kind: ServiceAccount
    name: {{ include "isolate-operator.serviceAccountName" . }}
    namespace: {{ .Release.Namespace }}
{{- end }}
"#
        .to_string()
    }

    /// Generate the helpers template.
    pub fn generate_helpers_template(&self) -> String {
        r#"{{/*
Expand the name of the chart.
*/}}
{{- define "isolate-operator.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "isolate-operator.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "isolate-operator.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "isolate-operator.labels" -}}
helm.sh/chart: {{ include "isolate-operator.chart" . }}
{{ include "isolate-operator.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "isolate-operator.selectorLabels" -}}
app.kubernetes.io/name: {{ include "isolate-operator.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "isolate-operator.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "isolate-operator.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}
"#
        .to_string()
    }

    /// Generate all chart files as a map.
    pub fn generate_all(&self) -> HashMap<String, String> {
        let mut files = HashMap::new();

        files.insert("Chart.yaml".to_string(), self.generate_chart_yaml());
        files.insert("values.yaml".to_string(), self.generate_values_yaml());
        files.insert(
            "templates/deployment.yaml".to_string(),
            self.generate_deployment_template(),
        );
        files.insert(
            "templates/service.yaml".to_string(),
            self.generate_service_template(),
        );
        files.insert(
            "templates/rbac.yaml".to_string(),
            self.generate_rbac_templates(),
        );
        files.insert(
            "templates/_helpers.tpl".to_string(),
            self.generate_helpers_template(),
        );
        files.insert(
            "crds/sandbox-crd.yaml".to_string(),
            super::generate_crd_yaml(),
        );

        files
    }
}

impl Default for HelmChartGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_metadata_default() {
        let metadata = ChartMetadata::default();
        assert_eq!(metadata.name, "isolate-operator");
        assert_eq!(metadata.api_version, "v2");
    }

    #[test]
    fn test_helm_values_default() {
        let values = HelmValues::default();
        assert_eq!(values.replica_count, 1);
        assert!(values.rbac.create);
        assert!(values.metrics.enabled);
    }

    #[test]
    fn test_generate_chart_yaml() {
        let generator = HelmChartGenerator::new();
        let yaml = generator.generate_chart_yaml();

        assert!(yaml.contains("apiVersion: v2"));
        assert!(yaml.contains("name: isolate-operator"));
        assert!(yaml.contains("type: application"));
    }

    #[test]
    fn test_generate_values_yaml() {
        let generator = HelmChartGenerator::new();
        let yaml = generator.generate_values_yaml();

        assert!(yaml.contains("replicaCount: 1"));
        assert!(yaml.contains("repository:"));
    }

    #[test]
    fn test_generate_deployment_template() {
        let generator = HelmChartGenerator::new();
        let template = generator.generate_deployment_template();

        assert!(template.contains("kind: Deployment"));
        assert!(template.contains(r#"{{ include "isolate-operator.fullname" . }}"#));
        assert!(template.contains("livenessProbe:"));
        assert!(template.contains("readinessProbe:"));
    }

    #[test]
    fn test_generate_rbac_templates() {
        let generator = HelmChartGenerator::new();
        let template = generator.generate_rbac_templates();

        assert!(template.contains("kind: ClusterRole"));
        assert!(template.contains("kind: ClusterRoleBinding"));
        assert!(template.contains("sandboxes"));
        assert!(template.contains("sandboxpools"));
    }

    #[test]
    fn test_generate_all() {
        let generator = HelmChartGenerator::new();
        let files = generator.generate_all();

        assert!(files.contains_key("Chart.yaml"));
        assert!(files.contains_key("values.yaml"));
        assert!(files.contains_key("templates/deployment.yaml"));
        assert!(files.contains_key("templates/service.yaml"));
        assert!(files.contains_key("templates/rbac.yaml"));
        assert!(files.contains_key("templates/_helpers.tpl"));
        assert!(files.contains_key("crds/sandbox-crd.yaml"));
    }
}
