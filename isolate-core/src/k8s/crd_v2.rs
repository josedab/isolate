//! V2 Custom Resource Definitions for the Isolate Kubernetes operator.
//!
//! Next-generation CRDs with enhanced configuration for multi-tenancy,
//! lifecycle management, observability, and fine-grained capabilities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// API version for v2 CRDs.
pub const API_VERSION_V2: &str = "isolate.io/v1beta1";

/// V2 Sandbox CRD with enhanced configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxV2 {
    pub api_version: String,
    pub kind: String,
    pub metadata: ResourceMetadata,
    pub spec: SandboxV2Spec,
    pub status: Option<SandboxV2Status>,
}

impl SandboxV2 {
    /// Create a new SandboxV2 with sensible defaults.
    pub fn new(name: &str, namespace: &str) -> Self {
        Self {
            api_version: API_VERSION_V2.to_string(),
            kind: "Sandbox".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                namespace: namespace.to_string(),
                ..Default::default()
            },
            spec: SandboxV2Spec {
                module: ModuleSpec {
                    source: ModuleSourceSpec::Image {
                        image: String::new(),
                        pull_policy: PullPolicy::IfNotPresent,
                    },
                    entry_point: "_start".to_string(),
                    arguments: Vec::new(),
                    environment: HashMap::new(),
                },
                resources: ResourceSpec {
                    memory_limit_mb: 128,
                    cpu_fuel: None,
                    io_read_limit_mb: None,
                    io_write_limit_mb: None,
                    timeout_secs: 30,
                    ephemeral_storage_mb: None,
                },
                capabilities: CapabilitySpec::default(),
                lifecycle: LifecycleSpec {
                    restart_policy: RestartPolicy::Never,
                    max_restarts: 3,
                    graceful_shutdown_secs: 30,
                    liveness_probe: None,
                    readiness_probe: None,
                },
                observability: ObservabilitySpec::default(),
                tenant: None,
                priority_class: None,
            },
            status: None,
        }
    }
}

/// Kubernetes resource metadata for v2 CRDs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceMetadata {
    pub name: String,
    pub namespace: String,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub owner_references: Vec<OwnerReference>,
    pub finalizers: Vec<String>,
    pub generation: u64,
}

/// Owner reference for Kubernetes garbage collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerReference {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub uid: String,
    pub controller: bool,
}

/// V2 Sandbox specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxV2Spec {
    pub module: ModuleSpec,
    pub resources: ResourceSpec,
    pub capabilities: CapabilitySpec,
    pub lifecycle: LifecycleSpec,
    pub observability: ObservabilitySpec,
    pub tenant: Option<String>,
    pub priority_class: Option<String>,
}

/// Module specification describing the WASM module to run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub source: ModuleSourceSpec,
    pub entry_point: String,
    pub arguments: Vec<String>,
    pub environment: HashMap<String, String>,
}

/// Source for the WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModuleSourceSpec {
    Image { image: String, pull_policy: PullPolicy },
    ConfigMap { name: String, key: String },
    Registry { name: String, version: String },
}

/// Image pull policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullPolicy {
    Always,
    IfNotPresent,
    Never,
}

/// Resource limits for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    pub memory_limit_mb: u64,
    pub cpu_fuel: Option<u64>,
    pub io_read_limit_mb: Option<u64>,
    pub io_write_limit_mb: Option<u64>,
    pub timeout_secs: u64,
    pub ephemeral_storage_mb: Option<u64>,
}

/// Capability grants for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub stdout: bool,
    pub stderr: bool,
    pub stdin: bool,
    pub filesystem: Vec<FilesystemMount>,
    pub network: NetworkSpec,
    pub time_access: bool,
    pub random: bool,
}

/// Filesystem mount for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemMount {
    pub path: String,
    pub read_only: bool,
    pub volume_source: VolumeSource,
}

/// Volume source for a filesystem mount.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolumeSource {
    EmptyDir,
    PersistentVolumeClaim { claim_name: String },
    ConfigMap { name: String },
    Secret { name: String },
}

/// Network configuration for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkSpec {
    pub allow_egress: bool,
    pub allowed_hosts: Vec<String>,
    pub dns_policy: DnsPolicy,
}

/// DNS resolution policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DnsPolicy {
    Default,
    None,
    ClusterFirst,
}

impl Default for DnsPolicy {
    fn default() -> Self {
        Self::ClusterFirst
    }
}

/// Lifecycle management for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleSpec {
    pub restart_policy: RestartPolicy,
    pub max_restarts: u32,
    pub graceful_shutdown_secs: u64,
    pub liveness_probe: Option<ProbeSpec>,
    pub readiness_probe: Option<ProbeSpec>,
}

/// Restart policy for a sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

/// Health probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub period_secs: u32,
    pub timeout_secs: u32,
    pub failure_threshold: u32,
    pub success_threshold: u32,
}

/// Observability configuration for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObservabilitySpec {
    pub metrics_enabled: bool,
    pub tracing_enabled: bool,
    pub log_level: String,
    pub audit_logging: bool,
}

/// SandboxV2 status (set by the controller).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxV2Status {
    pub phase: SandboxPhase,
    pub conditions: Vec<Condition>,
    pub restart_count: u32,
    pub last_execution: Option<ExecutionStatus>,
    pub resource_usage: Option<ResourceUsageStatus>,
    pub observed_generation: u64,
}

/// Phase of a sandbox in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxPhase {
    Pending,
    Compiling,
    Ready,
    Running,
    Succeeded,
    Failed,
    Terminating,
}

/// Status condition for a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub condition_type: String,
    pub status: String,
    pub reason: String,
    pub message: String,
    pub last_transition_time: String,
}

/// Execution status of the last sandbox run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStatus {
    pub exit_code: i32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Resource usage metrics for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsageStatus {
    pub memory_bytes: u64,
    pub fuel_consumed: u64,
    pub io_bytes_read: u64,
    pub io_bytes_written: u64,
}

/// Validate a SandboxV2Spec and return a list of validation errors.
pub fn validate_spec(spec: &SandboxV2Spec) -> Vec<String> {
    let mut errors = Vec::new();

    // Validate module source
    match &spec.module.source {
        ModuleSourceSpec::Image { image, .. } => {
            if image.is_empty() {
                errors.push("module.source.image must not be empty".to_string());
            }
        }
        ModuleSourceSpec::ConfigMap { name, key } => {
            if name.is_empty() {
                errors.push("module.source.configMap.name must not be empty".to_string());
            }
            if key.is_empty() {
                errors.push("module.source.configMap.key must not be empty".to_string());
            }
        }
        ModuleSourceSpec::Registry { name, version } => {
            if name.is_empty() {
                errors.push("module.source.registry.name must not be empty".to_string());
            }
            if version.is_empty() {
                errors.push("module.source.registry.version must not be empty".to_string());
            }
        }
    }

    // Validate entry point
    if spec.module.entry_point.is_empty() {
        errors.push("module.entry_point must not be empty".to_string());
    }

    // Validate resources
    if spec.resources.memory_limit_mb == 0 {
        errors.push("resources.memory_limit_mb must be greater than 0".to_string());
    }
    if spec.resources.timeout_secs == 0 {
        errors.push("resources.timeout_secs must be greater than 0".to_string());
    }

    // Validate lifecycle
    if spec.lifecycle.graceful_shutdown_secs == 0 {
        errors.push("lifecycle.graceful_shutdown_secs must be greater than 0".to_string());
    }

    // Validate filesystem mounts
    for (i, mount) in spec.capabilities.filesystem.iter().enumerate() {
        if mount.path.is_empty() {
            errors.push(format!("capabilities.filesystem[{}].path must not be empty", i));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_spec() -> SandboxV2Spec {
        SandboxV2Spec {
            module: ModuleSpec {
                source: ModuleSourceSpec::Image {
                    image: "ghcr.io/example/module:v1".to_string(),
                    pull_policy: PullPolicy::IfNotPresent,
                },
                entry_point: "_start".to_string(),
                arguments: vec![],
                environment: HashMap::new(),
            },
            resources: ResourceSpec {
                memory_limit_mb: 128,
                cpu_fuel: Some(1_000_000),
                io_read_limit_mb: None,
                io_write_limit_mb: None,
                timeout_secs: 30,
                ephemeral_storage_mb: None,
            },
            capabilities: CapabilitySpec::default(),
            lifecycle: LifecycleSpec {
                restart_policy: RestartPolicy::Never,
                max_restarts: 3,
                graceful_shutdown_secs: 30,
                liveness_probe: None,
                readiness_probe: None,
            },
            observability: ObservabilitySpec::default(),
            tenant: None,
            priority_class: None,
        }
    }

    #[test]
    fn test_api_version_v2() {
        assert_eq!(API_VERSION_V2, "isolate.io/v1beta1");
    }

    #[test]
    fn test_sandbox_v2_new() {
        let sb = SandboxV2::new("test-sandbox", "default");
        assert_eq!(sb.api_version, API_VERSION_V2);
        assert_eq!(sb.kind, "Sandbox");
        assert_eq!(sb.metadata.name, "test-sandbox");
        assert_eq!(sb.metadata.namespace, "default");
        assert!(sb.status.is_none());
    }

    #[test]
    fn test_sandbox_v2_defaults() {
        let sb = SandboxV2::new("defaults", "ns");
        assert_eq!(sb.spec.resources.memory_limit_mb, 128);
        assert_eq!(sb.spec.resources.timeout_secs, 30);
        assert_eq!(sb.spec.module.entry_point, "_start");
        assert_eq!(sb.spec.lifecycle.restart_policy, RestartPolicy::Never);
    }

    #[test]
    fn test_validate_spec_valid() {
        let spec = valid_spec();
        let errors = validate_spec(&spec);
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_spec_empty_image() {
        let mut spec = valid_spec();
        spec.module.source =
            ModuleSourceSpec::Image { image: String::new(), pull_policy: PullPolicy::Always };
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("image must not be empty")));
    }

    #[test]
    fn test_validate_spec_empty_configmap() {
        let mut spec = valid_spec();
        spec.module.source =
            ModuleSourceSpec::ConfigMap { name: String::new(), key: String::new() };
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("configMap.name")));
        assert!(errors.iter().any(|e| e.contains("configMap.key")));
    }

    #[test]
    fn test_validate_spec_empty_registry() {
        let mut spec = valid_spec();
        spec.module.source =
            ModuleSourceSpec::Registry { name: String::new(), version: String::new() };
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("registry.name")));
        assert!(errors.iter().any(|e| e.contains("registry.version")));
    }

    #[test]
    fn test_validate_spec_zero_memory() {
        let mut spec = valid_spec();
        spec.resources.memory_limit_mb = 0;
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("memory_limit_mb")));
    }

    #[test]
    fn test_validate_spec_zero_timeout() {
        let mut spec = valid_spec();
        spec.resources.timeout_secs = 0;
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("timeout_secs")));
    }

    #[test]
    fn test_validate_spec_empty_entry_point() {
        let mut spec = valid_spec();
        spec.module.entry_point = String::new();
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("entry_point")));
    }

    #[test]
    fn test_validate_spec_empty_mount_path() {
        let mut spec = valid_spec();
        spec.capabilities.filesystem.push(FilesystemMount {
            path: String::new(),
            read_only: true,
            volume_source: VolumeSource::EmptyDir,
        });
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("filesystem[0].path")));
    }

    #[test]
    fn test_validate_spec_zero_graceful_shutdown() {
        let mut spec = valid_spec();
        spec.lifecycle.graceful_shutdown_secs = 0;
        let errors = validate_spec(&spec);
        assert!(errors.iter().any(|e| e.contains("graceful_shutdown_secs")));
    }

    #[test]
    fn test_sandbox_v2_serialization() {
        let sb = SandboxV2::new("ser-test", "default");
        let json = serde_json::to_string(&sb).unwrap();
        assert!(json.contains("ser-test"));
        assert!(json.contains(API_VERSION_V2));
        let deserialized: SandboxV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.metadata.name, "ser-test");
    }

    #[test]
    fn test_sandbox_phase_values() {
        let phases = vec![
            SandboxPhase::Pending,
            SandboxPhase::Compiling,
            SandboxPhase::Ready,
            SandboxPhase::Running,
            SandboxPhase::Succeeded,
            SandboxPhase::Failed,
            SandboxPhase::Terminating,
        ];
        for phase in &phases {
            let json = serde_json::to_string(phase).unwrap();
            let back: SandboxPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, phase);
        }
    }

    #[test]
    fn test_dns_policy_default() {
        let policy = DnsPolicy::default();
        assert_eq!(policy, DnsPolicy::ClusterFirst);
    }

    #[test]
    fn test_resource_metadata_default() {
        let meta = ResourceMetadata::default();
        assert!(meta.name.is_empty());
        assert!(meta.labels.is_empty());
        assert!(meta.owner_references.is_empty());
        assert!(meta.finalizers.is_empty());
        assert_eq!(meta.generation, 0);
    }

    #[test]
    fn test_module_source_variants() {
        let img = ModuleSourceSpec::Image {
            image: "test:v1".to_string(),
            pull_policy: PullPolicy::Always,
        };
        let json = serde_json::to_string(&img).unwrap();
        assert!(json.contains("test:v1"));

        let cm = ModuleSourceSpec::ConfigMap {
            name: "my-cm".to_string(),
            key: "module.wasm".to_string(),
        };
        let json = serde_json::to_string(&cm).unwrap();
        assert!(json.contains("my-cm"));

        let reg = ModuleSourceSpec::Registry {
            name: "my-module".to_string(),
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&reg).unwrap();
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn test_volume_source_variants() {
        let sources = vec![
            VolumeSource::EmptyDir,
            VolumeSource::PersistentVolumeClaim { claim_name: "pvc-1".to_string() },
            VolumeSource::ConfigMap { name: "cm-1".to_string() },
            VolumeSource::Secret { name: "secret-1".to_string() },
        ];
        for source in &sources {
            let json = serde_json::to_string(source).unwrap();
            let back: VolumeSource = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&back).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_sandbox_v2_with_status() {
        let mut sb = SandboxV2::new("with-status", "default");
        sb.status = Some(SandboxV2Status {
            phase: SandboxPhase::Running,
            conditions: vec![Condition {
                condition_type: "Ready".to_string(),
                status: "True".to_string(),
                reason: "SandboxReady".to_string(),
                message: "Sandbox is ready".to_string(),
                last_transition_time: "2024-01-01T00:00:00Z".to_string(),
            }],
            restart_count: 0,
            last_execution: Some(ExecutionStatus {
                exit_code: 0,
                started_at: "2024-01-01T00:00:00Z".to_string(),
                finished_at: Some("2024-01-01T00:00:01Z".to_string()),
                duration_ms: Some(1000),
            }),
            resource_usage: Some(ResourceUsageStatus {
                memory_bytes: 1024,
                fuel_consumed: 500,
                io_bytes_read: 0,
                io_bytes_written: 100,
            }),
            observed_generation: 1,
        });

        let json = serde_json::to_string(&sb).unwrap();
        let back: SandboxV2 = serde_json::from_str(&json).unwrap();
        let status = back.status.unwrap();
        assert_eq!(status.phase, SandboxPhase::Running);
        assert_eq!(status.conditions.len(), 1);
        assert_eq!(status.last_execution.unwrap().exit_code, 0);
    }

    #[test]
    fn test_capability_spec_with_mounts_and_network() {
        let cap = CapabilitySpec {
            stdout: true,
            stderr: true,
            stdin: false,
            filesystem: vec![FilesystemMount {
                path: "/data".to_string(),
                read_only: false,
                volume_source: VolumeSource::PersistentVolumeClaim {
                    claim_name: "data-pvc".to_string(),
                },
            }],
            network: NetworkSpec {
                allow_egress: true,
                allowed_hosts: vec!["api.example.com".to_string()],
                dns_policy: DnsPolicy::ClusterFirst,
            },
            time_access: true,
            random: true,
        };

        let json = serde_json::to_string(&cap).unwrap();
        let back: CapabilitySpec = serde_json::from_str(&json).unwrap();
        assert!(back.stdout);
        assert_eq!(back.filesystem.len(), 1);
        assert!(back.network.allow_egress);
        assert_eq!(back.network.allowed_hosts, vec!["api.example.com"]);
    }
}
