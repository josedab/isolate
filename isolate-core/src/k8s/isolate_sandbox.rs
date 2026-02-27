//! IsolateSandbox CRD types, validation, and conversion helpers.
//!
//! Defines the `IsolateSandbox` Custom Resource with spec, status,
//! validation, capability parsing, and [`SandboxConfigBuilder`] conversion.

use crate::capability::Capability;
use crate::config::SandboxConfigBuilder;
use crate::sandbox_profile::SandboxProfile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Spec types
// ---------------------------------------------------------------------------

/// CRD spec — the desired state of an IsolateSandbox resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IsolateSandboxSpec {
    /// Where to load the WASM module from.
    pub module_source: IsolateModuleSource,
    /// Number of sandbox replicas.
    pub replicas: Option<u32>,
    /// Name of a [`SandboxProfile`] to apply (e.g. `"ai-code-execution"`).
    pub profile: Option<String>,
    /// Resource limits expressed in Kubernetes-friendly units.
    pub resources: Option<K8sResourceSpec>,
    /// Capability names to grant (e.g. `["stdout", "stderr"]`).
    pub capabilities: Option<Vec<String>>,
    /// Environment variables to inject.
    pub env: Option<Vec<IsolateEnvVar>>,
    /// Wall-clock timeout in seconds.
    pub timeout_seconds: Option<u64>,
    /// Optional horizontal auto-scaling configuration.
    pub auto_scaling: Option<AutoScalingSpec>,
}

/// Source of the WASM module bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum IsolateModuleSource {
    /// Base64-encoded WASM bytes inline in the spec.
    #[serde(rename_all = "camelCase")]
    Inline { wasm_base64: String },
    /// Reference to an OCI / WASM registry.
    #[serde(rename_all = "camelCase")]
    Registry { registry_url: String, module_name: String, version: String },
    /// Reference to a Kubernetes ConfigMap key.
    #[serde(rename_all = "camelCase")]
    ConfigMap { name: String, key: String },
}

/// Resource limits in Kubernetes-friendly units.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct K8sResourceSpec {
    /// Maximum heap memory in megabytes.
    pub memory_limit_mb: Option<u32>,
    /// CPU fuel units.
    pub cpu_fuel: Option<u64>,
    /// I/O write limit in kilobytes.
    pub io_write_limit_kb: Option<u64>,
    /// I/O read limit in kilobytes.
    pub io_read_limit_kb: Option<u64>,
}

/// Horizontal auto-scaling parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AutoScalingSpec {
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub target_cpu_percent: Option<u32>,
    pub target_memory_percent: Option<u32>,
    pub scale_down_delay_seconds: Option<u64>,
}

/// An environment variable definition (mirrors the Kubernetes API).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsolateEnvVar {
    pub name: String,
    pub value: Option<String>,
    #[serde(rename = "valueFrom")]
    pub value_from: Option<IsolateEnvVarSource>,
}

/// Source for a dynamically-resolved environment variable value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IsolateEnvVarSource {
    pub secret_ref: Option<IsolateSecretKeyRef>,
    pub config_map_ref: Option<IsolateConfigMapKeyRef>,
}

/// Reference to a key inside a Kubernetes Secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsolateSecretKeyRef {
    pub name: String,
    pub key: String,
}

/// Reference to a key inside a Kubernetes ConfigMap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsolateConfigMapKeyRef {
    pub name: String,
    pub key: String,
}

// ---------------------------------------------------------------------------
// Status types
// ---------------------------------------------------------------------------

/// Observed state of an IsolateSandbox resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IsolateSandboxStatus {
    pub phase: IsolateSandboxPhase,
    pub ready_replicas: u32,
    pub total_replicas: u32,
    pub conditions: Vec<IsolateSandboxCondition>,
    /// ISO 8601 timestamp of the last execution.
    pub last_execution: Option<String>,
    pub total_executions: u64,
    pub failed_executions: u64,
}

/// High-level lifecycle phase of an IsolateSandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IsolateSandboxPhase {
    Pending,
    Running,
    Failed,
    Terminated,
}

impl fmt::Display for IsolateSandboxPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Running => write!(f, "Running"),
            Self::Failed => write!(f, "Failed"),
            Self::Terminated => write!(f, "Terminated"),
        }
    }
}

/// A Kubernetes-style status condition for IsolateSandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IsolateSandboxCondition {
    pub condition_type: String,
    /// One of `"True"`, `"False"`, or `"Unknown"`.
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition_time: Option<String>,
}

// ---------------------------------------------------------------------------
// Full CRD wrapper
// ---------------------------------------------------------------------------

/// The full IsolateSandbox custom resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IsolateSandbox {
    pub api_version: String,
    pub kind: String,
    pub metadata: IsolateObjectMeta,
    pub spec: IsolateSandboxSpec,
    pub status: Option<IsolateSandboxStatus>,
}

/// Minimal Kubernetes ObjectMeta for IsolateSandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct IsolateObjectMeta {
    pub name: String,
    pub namespace: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub annotations: Option<HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

impl IsolateSandboxSpec {
    /// Validate the spec and return any errors found.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if let Some(r) = self.replicas {
            if r == 0 {
                errors.push("replicas must be > 0".to_string());
            }
        }

        if let Some(ref res) = self.resources {
            if let Some(mem) = res.memory_limit_mb {
                if mem == 0 {
                    errors.push("memory_limit_mb must be > 0".to_string());
                }
            }
        }

        if let Some(ref auto) = self.auto_scaling {
            if auto.min_replicas > auto.max_replicas {
                errors.push("auto_scaling min_replicas must be <= max_replicas".to_string());
            }
        }

        if let Some(t) = self.timeout_seconds {
            if t == 0 {
                errors.push("timeout_seconds must be > 0".to_string());
            }
        }

        if let Some(ref name) = self.profile {
            if self.resolve_profile().is_none() {
                errors.push(format!("unknown profile: '{}'", name));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Attempt to resolve the profile string to a [`SandboxProfile`].
    pub fn resolve_profile(&self) -> Option<SandboxProfile> {
        self.profile.as_ref().and_then(|name| name.parse::<SandboxProfile>().ok())
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Map a capability name to the corresponding [`Capability`] constructor.
pub fn parse_capability(name: &str) -> Option<Capability> {
    match name {
        "stdout" => Some(Capability::stdout()),
        "stderr" => Some(Capability::stderr()),
        "stdin" => Some(Capability::stdin()),
        "clock" => Some(Capability::system_clock()),
        "random" => Some(Capability::secure_random()),
        "temp_dir" => Some(Capability::temp_dir()),
        "env_all" => Some(Capability::env_all()),
        _ => None,
    }
}

impl IsolateSandboxSpec {
    /// Convert this spec into a [`SandboxConfigBuilder`].
    ///
    /// The builder still needs `.module()` called because [`IsolateModuleSource`]
    /// requires external resolution (fetch from registry, decode base64, etc.).
    pub fn to_sandbox_config_builder(&self) -> Result<SandboxConfigBuilder, String> {
        let mut builder = crate::SandboxConfig::builder();

        // Apply profile first so explicit settings can override.
        if let Some(profile) = self.resolve_profile() {
            builder = builder.use_profile(profile);
        }

        // Resource overrides.
        if let Some(ref res) = self.resources {
            if let Some(mem) = res.memory_limit_mb {
                builder = builder.memory_limit((mem as usize) * 1024 * 1024);
            }
            if let Some(fuel) = res.cpu_fuel {
                builder = builder.fuel(fuel);
            }
            if let Some(write_kb) = res.io_write_limit_kb {
                builder = builder.io_write_limit(write_kb * 1024);
            }
            if let Some(read_kb) = res.io_read_limit_kb {
                builder = builder.io_read_limit(read_kb * 1024);
            }
        }

        // Capabilities.
        if let Some(ref caps) = self.capabilities {
            for name in caps {
                if let Some(cap) = parse_capability(name) {
                    builder = builder.capability(cap);
                }
            }
        }

        // Environment variables (only literal values; valueFrom needs
        // external resolution by the operator).
        if let Some(ref env_vars) = self.env {
            for var in env_vars {
                if let Some(ref val) = var.value {
                    builder = builder.env(&var.name, val);
                }
            }
        }

        // Timeout.
        if let Some(secs) = self.timeout_seconds {
            builder = builder.wall_time_limit(Duration::from_secs(secs));
        }

        Ok(builder)
    }
}

// ---------------------------------------------------------------------------
// CRD YAML generation
// ---------------------------------------------------------------------------

/// Returns a static YAML string representing the CustomResourceDefinition
/// for the `IsolateSandbox` resource.
pub fn isolate_sandbox_crd_yaml() -> String {
    r#"apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: isolatesandboxes.isolate.dev
spec:
  group: isolate.dev
  names:
    kind: IsolateSandbox
    listKind: IsolateSandboxList
    plural: isolatesandboxes
    singular: isolatesandbox
    shortNames:
      - isb
  scope: Namespaced
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
                moduleSource:
                  type: object
                replicas:
                  type: integer
                profile:
                  type: string
                resources:
                  type: object
                capabilities:
                  type: array
                  items:
                    type: string
                timeoutSeconds:
                  type: integer
            status:
              type: object
              properties:
                phase:
                  type: string
                readyReplicas:
                  type: integer
                totalReplicas:
                  type: integer
      subresources:
        status: {}
"#
    .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> IsolateSandboxSpec {
        IsolateSandboxSpec {
            module_source: IsolateModuleSource::Inline { wasm_base64: "AGFzbQEAAAA=".to_string() },
            replicas: Some(2),
            profile: Some("ai-code-execution".to_string()),
            resources: Some(K8sResourceSpec {
                memory_limit_mb: Some(64),
                cpu_fuel: Some(500_000),
                io_write_limit_kb: Some(1024),
                io_read_limit_kb: Some(2048),
            }),
            capabilities: Some(vec!["stdout".to_string(), "stderr".to_string()]),
            env: Some(vec![IsolateEnvVar {
                name: "MODE".to_string(),
                value: Some("production".to_string()),
                value_from: None,
            }]),
            timeout_seconds: Some(30),
            auto_scaling: None,
        }
    }

    fn sample_sandbox() -> IsolateSandbox {
        IsolateSandbox {
            api_version: "isolate.dev/v1alpha1".to_string(),
            kind: "IsolateSandbox".to_string(),
            metadata: IsolateObjectMeta {
                name: "my-sandbox".to_string(),
                namespace: Some("default".to_string()),
                labels: Some(HashMap::from([("app".to_string(), "demo".to_string())])),
                annotations: None,
            },
            spec: sample_spec(),
            status: None,
        }
    }

    // -- Serialize / deserialize roundtrip --

    #[test]
    fn test_serialize_deserialize_isolate_sandbox() {
        let sandbox = sample_sandbox();
        let json = serde_json::to_string_pretty(&sandbox).expect("serialize");
        let deserialized: IsolateSandbox = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(sandbox, deserialized);
    }

    // -- Validation: valid spec --

    #[test]
    fn test_validate_valid_spec() {
        let spec = sample_spec();
        assert!(spec.validate().is_ok());
    }

    // -- Validation: invalid spec --

    #[test]
    fn test_validate_invalid_replicas_zero() {
        let mut spec = sample_spec();
        spec.replicas = Some(0);
        let errs = spec.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("replicas")));
    }

    #[test]
    fn test_validate_invalid_autoscaling() {
        let mut spec = sample_spec();
        spec.auto_scaling = Some(AutoScalingSpec {
            min_replicas: 5,
            max_replicas: 2,
            target_cpu_percent: None,
            target_memory_percent: None,
            scale_down_delay_seconds: None,
        });
        let errs = spec.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("min_replicas")));
    }

    #[test]
    fn test_validate_invalid_memory_zero() {
        let mut spec = sample_spec();
        spec.resources = Some(K8sResourceSpec {
            memory_limit_mb: Some(0),
            cpu_fuel: None,
            io_write_limit_kb: None,
            io_read_limit_kb: None,
        });
        let errs = spec.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("memory_limit_mb")));
    }

    #[test]
    fn test_validate_invalid_timeout_zero() {
        let mut spec = sample_spec();
        spec.timeout_seconds = Some(0);
        let errs = spec.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("timeout_seconds")));
    }

    #[test]
    fn test_validate_unknown_profile() {
        let mut spec = sample_spec();
        spec.profile = Some("does-not-exist".to_string());
        let errs = spec.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("unknown profile")));
    }

    // -- resolve_profile --

    #[test]
    fn test_resolve_profile_valid_names() {
        let names_and_expected = [
            ("ai-code-execution", SandboxProfile::AiCodeExecution),
            ("plugin-runtime", SandboxProfile::PluginRuntime),
            ("ci-runner", SandboxProfile::CiRunner),
            ("edge-function", SandboxProfile::EdgeFunction),
            ("playground", SandboxProfile::Playground),
            ("unrestricted", SandboxProfile::Unrestricted),
        ];
        for (name, expected) in &names_and_expected {
            let spec = IsolateSandboxSpec {
                module_source: IsolateModuleSource::Inline { wasm_base64: String::new() },
                replicas: None,
                profile: Some(name.to_string()),
                resources: None,
                capabilities: None,
                env: None,
                timeout_seconds: None,
                auto_scaling: None,
            };
            assert_eq!(spec.resolve_profile(), Some(expected.clone()));
        }
    }

    #[test]
    fn test_resolve_profile_unknown_returns_none() {
        let spec = IsolateSandboxSpec {
            module_source: IsolateModuleSource::Inline { wasm_base64: String::new() },
            replicas: None,
            profile: Some("nope".to_string()),
            resources: None,
            capabilities: None,
            env: None,
            timeout_seconds: None,
            auto_scaling: None,
        };
        assert_eq!(spec.resolve_profile(), None);
    }

    #[test]
    fn test_resolve_profile_none_returns_none() {
        let spec = IsolateSandboxSpec {
            module_source: IsolateModuleSource::Inline { wasm_base64: String::new() },
            replicas: None,
            profile: None,
            resources: None,
            capabilities: None,
            env: None,
            timeout_seconds: None,
            auto_scaling: None,
        };
        assert_eq!(spec.resolve_profile(), None);
    }

    // -- parse_capability --

    #[test]
    fn test_parse_capability_known() {
        assert_eq!(parse_capability("stdout"), Some(Capability::stdout()));
        assert_eq!(parse_capability("stderr"), Some(Capability::stderr()));
        assert_eq!(parse_capability("stdin"), Some(Capability::stdin()));
        assert_eq!(parse_capability("clock"), Some(Capability::system_clock()));
        assert_eq!(parse_capability("random"), Some(Capability::secure_random()));
        assert_eq!(parse_capability("temp_dir"), Some(Capability::temp_dir()));
        assert_eq!(parse_capability("env_all"), Some(Capability::env_all()));
    }

    #[test]
    fn test_parse_capability_unknown_returns_none() {
        assert_eq!(parse_capability("unknown_cap"), None);
        assert_eq!(parse_capability(""), None);
    }

    // -- to_sandbox_config_builder --

    #[test]
    fn test_to_sandbox_config_builder_with_profile() {
        let spec = IsolateSandboxSpec {
            module_source: IsolateModuleSource::Inline { wasm_base64: String::new() },
            replicas: None,
            profile: Some("ai-code-execution".to_string()),
            resources: None,
            capabilities: None,
            env: None,
            timeout_seconds: None,
            auto_scaling: None,
        };
        let builder = spec.to_sandbox_config_builder().expect("builder");

        let wasm: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let config = builder.module(wasm).expect("module").build().expect("build");

        assert_eq!(config.resources.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(100_000));
    }

    #[test]
    fn test_to_sandbox_config_builder_with_resources() {
        let spec = IsolateSandboxSpec {
            module_source: IsolateModuleSource::Inline { wasm_base64: String::new() },
            replicas: None,
            profile: None,
            resources: Some(K8sResourceSpec {
                memory_limit_mb: Some(128),
                cpu_fuel: Some(999),
                io_write_limit_kb: Some(512),
                io_read_limit_kb: Some(1024),
            }),
            capabilities: Some(vec!["stdout".to_string()]),
            env: Some(vec![IsolateEnvVar {
                name: "FOO".to_string(),
                value: Some("bar".to_string()),
                value_from: None,
            }]),
            timeout_seconds: Some(60),
            auto_scaling: None,
        };

        let builder = spec.to_sandbox_config_builder().expect("builder");
        let wasm: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let config = builder.module(wasm).expect("module").build().expect("build");

        assert_eq!(config.resources.memory.heap_max, 128 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(999));
        assert_eq!(config.resources.io.write_bytes, Some(512 * 1024));
        assert_eq!(config.resources.io.read_bytes, Some(1024 * 1024));
        assert!(config.capabilities.has(&Capability::stdout()));
        assert_eq!(config.env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(config.resources.time.wall_time, Some(Duration::from_secs(60)));
    }

    // -- ModuleSource serialization --

    #[test]
    fn test_module_source_inline_serialization() {
        let src = IsolateModuleSource::Inline { wasm_base64: "AAAA".to_string() };
        let json = serde_json::to_string(&src).expect("serialize");
        assert!(json.contains("\"type\":\"inline\""));
        let back: IsolateModuleSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(src, back);
    }

    #[test]
    fn test_module_source_registry_serialization() {
        let src = IsolateModuleSource::Registry {
            registry_url: "https://registry.example.com".to_string(),
            module_name: "my-module".to_string(),
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&src).expect("serialize");
        assert!(json.contains("\"type\":\"registry\""));
        let back: IsolateModuleSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(src, back);
    }

    #[test]
    fn test_module_source_configmap_serialization() {
        let src = IsolateModuleSource::ConfigMap {
            name: "my-configmap".to_string(),
            key: "module.wasm".to_string(),
        };
        let json = serde_json::to_string(&src).expect("serialize");
        assert!(json.contains("\"type\":\"configMap\""));
        let back: IsolateModuleSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(src, back);
    }

    // -- SandboxPhase Display --

    #[test]
    fn test_sandbox_phase_display() {
        assert_eq!(IsolateSandboxPhase::Pending.to_string(), "Pending");
        assert_eq!(IsolateSandboxPhase::Running.to_string(), "Running");
        assert_eq!(IsolateSandboxPhase::Failed.to_string(), "Failed");
        assert_eq!(IsolateSandboxPhase::Terminated.to_string(), "Terminated");
    }

    // -- Status with conditions --

    #[test]
    fn test_status_with_conditions() {
        let status = IsolateSandboxStatus {
            phase: IsolateSandboxPhase::Running,
            ready_replicas: 2,
            total_replicas: 3,
            conditions: vec![
                IsolateSandboxCondition {
                    condition_type: "Available".to_string(),
                    status: "True".to_string(),
                    reason: Some("MinimumReplicasAvailable".to_string()),
                    message: Some("2/3 replicas ready".to_string()),
                    last_transition_time: Some("2024-01-15T10:30:00Z".to_string()),
                },
                IsolateSandboxCondition {
                    condition_type: "Progressing".to_string(),
                    status: "True".to_string(),
                    reason: None,
                    message: None,
                    last_transition_time: None,
                },
            ],
            last_execution: Some("2024-01-15T10:29:55Z".to_string()),
            total_executions: 1000,
            failed_executions: 5,
        };

        let json = serde_json::to_string_pretty(&status).expect("serialize");
        let back: IsolateSandboxStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, back);
        assert_eq!(back.conditions.len(), 2);
        assert_eq!(back.conditions[0].condition_type, "Available");
    }

    // -- crd_yaml --

    #[test]
    fn test_crd_yaml_is_non_empty_and_contains_expected_strings() {
        let yaml = isolate_sandbox_crd_yaml();
        assert!(!yaml.is_empty());
        assert!(yaml.contains("CustomResourceDefinition"));
        assert!(yaml.contains("IsolateSandbox"));
        assert!(yaml.contains("isolate.dev"));
        assert!(yaml.contains("v1alpha1"));
        assert!(yaml.contains("isolatesandboxes"));
    }

    // -- EnvVar with valueFrom --

    #[test]
    fn test_env_var_with_value_from() {
        let env = IsolateEnvVar {
            name: "DB_PASSWORD".to_string(),
            value: None,
            value_from: Some(IsolateEnvVarSource {
                secret_ref: Some(IsolateSecretKeyRef {
                    name: "db-secret".to_string(),
                    key: "password".to_string(),
                }),
                config_map_ref: None,
            }),
        };

        let json = serde_json::to_string(&env).expect("serialize");
        let back: IsolateEnvVar = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(env, back);
        assert!(back.value.is_none());
        assert!(back.value_from.is_some());
        let src = back.value_from.unwrap();
        assert_eq!(src.secret_ref.unwrap().name, "db-secret");
    }

    #[test]
    fn test_env_var_with_config_map_ref() {
        let env = IsolateEnvVar {
            name: "APP_CONFIG".to_string(),
            value: None,
            value_from: Some(IsolateEnvVarSource {
                secret_ref: None,
                config_map_ref: Some(IsolateConfigMapKeyRef {
                    name: "app-config".to_string(),
                    key: "settings.json".to_string(),
                }),
            }),
        };

        let json = serde_json::to_string(&env).expect("serialize");
        let back: IsolateEnvVar = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(env, back);
    }
}
