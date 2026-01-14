//! Kubernetes Operator Integration
//!
//! Custom Resource Definitions and controllers for Kubernetes:
//! - Sandbox CRD for declarative sandbox management
//! - SandboxPool CRD for auto-scaling pools
//! - SandboxPolicy CRD for cluster-wide policies
//! - Resource-aware scheduling
//! - Operator reconciliation with retries
//! - Helm chart generation for deployment
//! - Health checks and readiness probes

pub mod autoscaler;
pub mod crd_v2;
pub mod disaster_recovery;
pub mod helm;
pub mod network_policy;
pub mod operator;
pub mod scheduler;
pub mod tenant;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export scheduler types
pub use scheduler::{
    AntiAffinityTerm, LabelExpression, LabelOperator, NodeAffinity, NodeResources, PodAntiAffinity,
    PreferredExpression, ResourceRequest, SandboxScheduler, SchedulingDecision, SchedulingStrategy,
    Taint, TaintEffect, Toleration, TolerationOperator, WeightedAntiAffinityTerm,
};

// Re-export operator types
pub use operator::{
    EventType, IsolateOperator, OperatorConfig, OperatorEvent, OperatorMetrics, OperatorState,
    ReconcileAction,
};

// Re-export helm types
pub use helm::{
    ChartMetadata, HelmChartGenerator, HelmValues, ImageConfig, Maintainer, MetricsConfig,
    OperatorValues, RbacConfig, ResourceConfig, ResourceLimits, SecurityContext,
    ServiceAccountConfig, ServiceConfig, ServiceMonitorConfig, TolerationConfig,
};

// Re-export network policy types
pub use network_policy::{
    generate_admission_webhook, generate_network_policy, generate_pdb, NetworkPolicy,
    NetworkPolicySpec, PodDisruptionBudget, ValidatingWebhookConfiguration,
};

// Re-export v2 CRD types
pub use crd_v2::{
    validate_spec, CapabilitySpec, LifecycleSpec, ModuleSourceSpec, ModuleSpec, ObservabilitySpec,
    ResourceMetadata, ResourceSpec, SandboxV2, SandboxV2Spec, SandboxV2Status, API_VERSION_V2,
};

// Re-export tenant types
pub use tenant::{
    IsolationLevel, Permission, QuotaExceeded, Tenant, TenantManager, TenantQuota, TenantRole,
    TenantStatus, TenantUsage,
};

// Re-export disaster recovery types
pub use disaster_recovery::{
    Backup, BackupContents, BackupStatus, BackupType, ClusterHealth, DisasterRecoveryManager,
    FailoverConfig, FailoverStatus, FailoverStrategy, RestoreOperation, RestoreStatus, RestoreType,
};

/// Kubernetes API version for Isolate CRDs.
pub const API_VERSION: &str = "isolate.io/v1alpha1";

/// Sandbox Custom Resource Definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCrd {
    /// API version.
    pub api_version: String,
    /// Kind.
    pub kind: String,
    /// Metadata.
    pub metadata: ObjectMeta,
    /// Spec.
    pub spec: SandboxSpec,
    /// Status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SandboxStatus>,
}

impl SandboxCrd {
    /// Create a new Sandbox CRD.
    pub fn new(name: &str, namespace: &str, spec: SandboxSpec) -> Self {
        Self {
            api_version: API_VERSION.to_string(),
            kind: "Sandbox".to_string(),
            metadata: ObjectMeta {
                name: name.to_string(),
                namespace: Some(namespace.to_string()),
                labels: HashMap::new(),
                annotations: HashMap::new(),
                uid: None,
                resource_version: None,
            },
            spec,
            status: None,
        }
    }
}

/// Kubernetes object metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMeta {
    /// Name.
    pub name: String,
    /// Namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Labels.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Annotations.
    #[serde(default)]
    pub annotations: HashMap<String, String>,
    /// UID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// Resource version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_version: Option<String>,
}

/// Sandbox specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSpec {
    /// WASM module source.
    pub module: ModuleSource,
    /// Resource limits.
    #[serde(default)]
    pub resources: ResourceRequirements,
    /// Capabilities to grant.
    #[serde(default)]
    pub capabilities: Vec<CapabilityGrant>,
    /// Environment variables.
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Secrets to mount.
    #[serde(default)]
    pub secrets: Vec<SecretMount>,
    /// Timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Replicas (for pool mode).
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    /// Scaling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<ScalingSpec>,
    /// Health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<HealthCheck>,
    /// Service configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceSpec>,
}

fn default_replicas() -> u32 {
    1
}

/// Module source specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSource {
    /// OCI image reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// ConfigMap reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_map: Option<ConfigMapRef>,
    /// Inline base64-encoded WASM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline: Option<String>,
    /// HTTP URL to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Image pull policy.
    #[serde(default)]
    pub pull_policy: PullPolicy,
}

/// ConfigMap reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMapRef {
    /// ConfigMap name.
    pub name: String,
    /// Key within ConfigMap.
    pub key: String,
}

/// Image pull policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullPolicy {
    #[default]
    IfNotPresent,
    Always,
    Never,
}

/// Resource requirements.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRequirements {
    /// Resource limits.
    #[serde(default)]
    pub limits: ResourceList,
    /// Resource requests.
    #[serde(default)]
    pub requests: ResourceList,
}

/// Resource list.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceList {
    /// Memory (e.g., "128Mi").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// CPU (e.g., "100m").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    /// Fuel (execution units).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
    /// I/O bandwidth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_bandwidth: Option<String>,
}

/// Capability grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityGrant {
    /// Capability type.
    #[serde(rename = "type")]
    pub cap_type: String,
    /// Allowed targets (hosts, paths, etc.).
    #[serde(default)]
    pub allow: Vec<String>,
    /// Denied targets.
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Environment variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    /// Variable name.
    pub name: String,
    /// Direct value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Value from source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_from: Option<EnvVarSource>,
}

/// Environment variable source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVarSource {
    /// Secret key reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_ref: Option<SecretKeyRef>,
    /// ConfigMap key reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_map_key_ref: Option<ConfigMapKeyRef>,
}

/// Secret key reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyRef {
    /// Secret name.
    pub name: String,
    /// Key.
    pub key: String,
}

/// ConfigMap key reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMapKeyRef {
    /// ConfigMap name.
    pub name: String,
    /// Key.
    pub key: String,
}

/// Secret mount.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMount {
    /// Secret name.
    pub name: String,
    /// Mount path in sandbox filesystem.
    pub mount_path: String,
    /// Read only.
    #[serde(default = "default_true")]
    pub read_only: bool,
}

fn default_true() -> bool {
    true
}

/// Scaling specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalingSpec {
    /// Minimum replicas.
    pub min_replicas: u32,
    /// Maximum replicas.
    pub max_replicas: u32,
    /// Target CPU utilization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_cpu_utilization: Option<u32>,
    /// Target memory utilization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_memory_utilization: Option<u32>,
    /// Scale down delay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_down_delay_seconds: Option<u32>,
}

/// Health check specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    /// Function to call for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// HTTP endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_get: Option<HttpHealthCheck>,
    /// Initial delay.
    #[serde(default = "default_initial_delay")]
    pub initial_delay_seconds: u32,
    /// Period between checks.
    #[serde(default = "default_period")]
    pub period_seconds: u32,
    /// Failure threshold.
    #[serde(default = "default_threshold")]
    pub failure_threshold: u32,
    /// Success threshold.
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
}

fn default_initial_delay() -> u32 {
    5
}

fn default_period() -> u32 {
    10
}

fn default_threshold() -> u32 {
    3
}

fn default_success_threshold() -> u32 {
    1
}

/// HTTP health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpHealthCheck {
    /// Path.
    pub path: String,
    /// Port.
    pub port: u16,
}

/// Service specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    /// Service type.
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    /// Ports.
    pub ports: Vec<ServicePort>,
}

/// Service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
}

/// Service port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePort {
    /// Port name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Port number.
    pub port: u16,
    /// Target port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    /// Protocol.
    #[serde(default)]
    pub protocol: PortProtocol,
}

/// Port protocol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortProtocol {
    #[default]
    TCP,
    UDP,
}

/// Sandbox status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// Current phase.
    pub phase: SandboxPhase,
    /// Ready replicas.
    pub ready_replicas: u32,
    /// Available replicas.
    pub available_replicas: u32,
    /// Conditions.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Last execution time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_execution_time: Option<String>,
    /// Message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Sandbox phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxPhase {
    #[default]
    Pending,
    Initializing,
    Running,
    Suspended,
    Failed,
    Terminated,
}

/// Condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Condition type.
    #[serde(rename = "type")]
    pub condition_type: String,
    /// Status (True, False, Unknown).
    pub status: String,
    /// Last transition time.
    pub last_transition_time: String,
    /// Reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// SandboxPool CRD for managing pools of sandboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPoolCrd {
    /// API version.
    pub api_version: String,
    /// Kind.
    pub kind: String,
    /// Metadata.
    pub metadata: ObjectMeta,
    /// Spec.
    pub spec: SandboxPoolSpec,
    /// Status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SandboxPoolStatus>,
}

/// SandboxPool specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPoolSpec {
    /// Sandbox template.
    pub template: SandboxSpec,
    /// Pool size.
    pub size: u32,
    /// Warm pool size.
    pub warm_pool_size: u32,
    /// Max idle time before recycling.
    pub max_idle_seconds: u32,
    /// Preload on startup.
    #[serde(default)]
    pub preload: bool,
}

/// SandboxPool status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxPoolStatus {
    /// Total sandboxes.
    pub total: u32,
    /// Ready sandboxes.
    pub ready: u32,
    /// In-use sandboxes.
    pub in_use: u32,
    /// Warm sandboxes.
    pub warm: u32,
}

/// Controller for reconciling Sandbox resources.
pub struct SandboxController {
    namespace: String,
}

impl SandboxController {
    /// Create a new controller.
    pub fn new(namespace: &str) -> Self {
        Self { namespace: namespace.to_string() }
    }

    /// Reconcile a Sandbox resource.
    pub fn reconcile(&self, sandbox: &mut SandboxCrd) -> ReconcileResult {
        let status = sandbox.status.get_or_insert_with(SandboxStatus::default);

        match status.phase {
            SandboxPhase::Pending => {
                // Validate spec
                if let Err(e) = self.validate_spec(&sandbox.spec) {
                    status.phase = SandboxPhase::Failed;
                    status.message = Some(e);
                    return ReconcileResult::Failed;
                }

                status.phase = SandboxPhase::Initializing;
                ReconcileResult::Requeue
            }
            SandboxPhase::Initializing => {
                // Initialize sandbox
                status.phase = SandboxPhase::Running;
                status.ready_replicas = sandbox.spec.replicas;
                status.available_replicas = sandbox.spec.replicas;
                status.conditions.push(Condition {
                    condition_type: "Ready".to_string(),
                    status: "True".to_string(),
                    last_transition_time: chrono_now(),
                    reason: Some("SandboxReady".to_string()),
                    message: Some("Sandbox is ready".to_string()),
                });
                ReconcileResult::Ok
            }
            SandboxPhase::Running => {
                // Check health
                ReconcileResult::Ok
            }
            SandboxPhase::Suspended => ReconcileResult::Ok,
            SandboxPhase::Failed => ReconcileResult::Failed,
            SandboxPhase::Terminated => ReconcileResult::Ok,
        }
    }

    fn validate_spec(&self, spec: &SandboxSpec) -> Result<(), String> {
        if spec.module.image.is_none()
            && spec.module.config_map.is_none()
            && spec.module.inline.is_none()
            && spec.module.url.is_none()
        {
            return Err("No module source specified".to_string());
        }
        Ok(())
    }

    /// Get namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

/// Reconcile result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileResult {
    /// Success.
    Ok,
    /// Requeue for another reconcile.
    Requeue,
    /// Failed.
    Failed,
}

/// Generate CRD YAML.
pub fn generate_crd_yaml() -> String {
    r#"apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: sandboxes.isolate.io
spec:
  group: isolate.io
  versions:
    - name: v1alpha1
      served: true
      storage: true
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              required:
                - module
              properties:
                module:
                  type: object
                  properties:
                    image:
                      type: string
                    configMap:
                      type: object
                      properties:
                        name:
                          type: string
                        key:
                          type: string
                    url:
                      type: string
                resources:
                  type: object
                  properties:
                    limits:
                      type: object
                      properties:
                        memory:
                          type: string
                        cpu:
                          type: string
                        fuel:
                          type: integer
                    requests:
                      type: object
                capabilities:
                  type: array
                  items:
                    type: object
                env:
                  type: array
                  items:
                    type: object
                replicas:
                  type: integer
                  default: 1
                timeoutSeconds:
                  type: integer
            status:
              type: object
              properties:
                phase:
                  type: string
                readyReplicas:
                  type: integer
                availableReplicas:
                  type: integer
                conditions:
                  type: array
                  items:
                    type: object
      subresources:
        status: {}
        scale:
          specReplicasPath: .spec.replicas
          statusReplicasPath: .status.readyReplicas
  scope: Namespaced
  names:
    plural: sandboxes
    singular: sandbox
    kind: Sandbox
    shortNames:
      - sb
---
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: sandboxpools.isolate.io
spec:
  group: isolate.io
  versions:
    - name: v1alpha1
      served: true
      storage: true
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              properties:
                template:
                  type: object
                size:
                  type: integer
                warmPoolSize:
                  type: integer
                maxIdleSeconds:
                  type: integer
            status:
              type: object
      subresources:
        status: {}
  scope: Namespaced
  names:
    plural: sandboxpools
    singular: sandboxpool
    kind: SandboxPool
    shortNames:
      - sbp
"#
    .to_string()
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{}Z", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_crd_creation() {
        let spec = SandboxSpec {
            module: ModuleSource {
                image: Some("ghcr.io/example/module:v1".to_string()),
                config_map: None,
                inline: None,
                url: None,
                pull_policy: PullPolicy::IfNotPresent,
            },
            resources: ResourceRequirements::default(),
            capabilities: vec![],
            env: vec![],
            secrets: vec![],
            timeout_seconds: Some(30),
            replicas: 1,
            scaling: None,
            health_check: None,
            service: None,
        };

        let crd = SandboxCrd::new("my-sandbox", "default", spec);
        assert_eq!(crd.kind, "Sandbox");
        assert_eq!(crd.metadata.name, "my-sandbox");
    }

    #[test]
    fn test_serialize_crd() {
        let spec = SandboxSpec {
            module: ModuleSource {
                image: Some("example:v1".to_string()),
                config_map: None,
                inline: None,
                url: None,
                pull_policy: PullPolicy::Always,
            },
            resources: ResourceRequirements {
                limits: ResourceList {
                    memory: Some("128Mi".to_string()),
                    cpu: Some("100m".to_string()),
                    fuel: Some(1_000_000),
                    io_bandwidth: None,
                },
                requests: ResourceList::default(),
            },
            capabilities: vec![CapabilityGrant {
                cap_type: "network".to_string(),
                allow: vec!["api.example.com".to_string()],
                deny: vec![],
            }],
            env: vec![EnvVar {
                name: "API_KEY".to_string(),
                value: None,
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeyRef {
                        name: "api-secrets".to_string(),
                        key: "api-key".to_string(),
                    }),
                    config_map_key_ref: None,
                }),
            }],
            secrets: vec![],
            timeout_seconds: None,
            replicas: 3,
            scaling: Some(ScalingSpec {
                min_replicas: 1,
                max_replicas: 10,
                target_cpu_utilization: Some(80),
                target_memory_utilization: None,
                scale_down_delay_seconds: Some(300),
            }),
            health_check: Some(HealthCheck {
                function: Some("health".to_string()),
                http_get: None,
                initial_delay_seconds: 5,
                period_seconds: 10,
                failure_threshold: 3,
                success_threshold: 1,
            }),
            service: None,
        };

        let crd = SandboxCrd::new("test", "default", spec);
        let yaml = serde_json::to_string_pretty(&crd).unwrap();
        assert!(yaml.contains("example:v1"));
    }

    #[test]
    fn test_controller_reconcile() {
        let controller = SandboxController::new("default");
        let spec = SandboxSpec {
            module: ModuleSource {
                image: Some("test:v1".to_string()),
                config_map: None,
                inline: None,
                url: None,
                pull_policy: PullPolicy::default(),
            },
            resources: ResourceRequirements::default(),
            capabilities: vec![],
            env: vec![],
            secrets: vec![],
            timeout_seconds: None,
            replicas: 1,
            scaling: None,
            health_check: None,
            service: None,
        };

        let mut crd = SandboxCrd::new("test", "default", spec);

        // First reconcile: Pending -> Initializing
        let result = controller.reconcile(&mut crd);
        assert_eq!(result, ReconcileResult::Requeue);
        assert_eq!(crd.status.as_ref().unwrap().phase, SandboxPhase::Initializing);

        // Second reconcile: Initializing -> Running
        let result = controller.reconcile(&mut crd);
        assert_eq!(result, ReconcileResult::Ok);
        assert_eq!(crd.status.as_ref().unwrap().phase, SandboxPhase::Running);
    }

    #[test]
    fn test_controller_validation_failure() {
        let controller = SandboxController::new("default");
        let spec = SandboxSpec {
            module: ModuleSource {
                image: None,
                config_map: None,
                inline: None,
                url: None,
                pull_policy: PullPolicy::default(),
            },
            resources: ResourceRequirements::default(),
            capabilities: vec![],
            env: vec![],
            secrets: vec![],
            timeout_seconds: None,
            replicas: 1,
            scaling: None,
            health_check: None,
            service: None,
        };

        let mut crd = SandboxCrd::new("test", "default", spec);
        let result = controller.reconcile(&mut crd);

        assert_eq!(result, ReconcileResult::Failed);
        assert_eq!(crd.status.as_ref().unwrap().phase, SandboxPhase::Failed);
    }

    #[test]
    fn test_generate_crd_yaml() {
        let yaml = generate_crd_yaml();
        assert!(yaml.contains("apiextensions.k8s.io/v1"));
        assert!(yaml.contains("sandboxes.isolate.io"));
        assert!(yaml.contains("sandboxpools.isolate.io"));
    }

    #[test]
    fn test_sandbox_pool_crd() {
        let pool = SandboxPoolCrd {
            api_version: API_VERSION.to_string(),
            kind: "SandboxPool".to_string(),
            metadata: ObjectMeta {
                name: "my-pool".to_string(),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: SandboxPoolSpec {
                template: SandboxSpec {
                    module: ModuleSource {
                        image: Some("test:v1".to_string()),
                        config_map: None,
                        inline: None,
                        url: None,
                        pull_policy: PullPolicy::default(),
                    },
                    resources: ResourceRequirements::default(),
                    capabilities: vec![],
                    env: vec![],
                    secrets: vec![],
                    timeout_seconds: None,
                    replicas: 1,
                    scaling: None,
                    health_check: None,
                    service: None,
                },
                size: 10,
                warm_pool_size: 5,
                max_idle_seconds: 300,
                preload: true,
            },
            status: Some(SandboxPoolStatus { total: 10, ready: 8, in_use: 3, warm: 5 }),
        };

        assert_eq!(pool.spec.size, 10);
        assert_eq!(pool.status.unwrap().ready, 8);
    }
}
