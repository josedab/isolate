//! IaC resource definitions for Isolate.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of resources that can be managed via IaC.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    Sandbox,
    CapabilityPolicy,
    SandboxPool,
    Module,
    SecretStore,
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceType::Sandbox => write!(f, "sandbox"),
            ResourceType::CapabilityPolicy => write!(f, "capability_policy"),
            ResourceType::SandboxPool => write!(f, "sandbox_pool"),
            ResourceType::Module => write!(f, "module"),
            ResourceType::SecretStore => write!(f, "secret_store"),
        }
    }
}

/// A resource managed by an IaC provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IacResource {
    pub resource_type: ResourceType,
    pub name: String,
    /// Assigned after creation.
    pub id: Option<String>,
    pub properties: HashMap<String, serde_json::Value>,
    pub labels: HashMap<String, String>,
    /// Resource names this resource depends on.
    pub depends_on: Vec<String>,
    pub lifecycle: LifecyclePolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LifecyclePolicy {
    pub prevent_destroy: bool,
    pub create_before_destroy: bool,
    /// Field names to ignore during diff.
    pub ignore_changes: Vec<String>,
}

/// Sandbox resource properties (type-safe wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResource {
    pub name: String,
    /// Path or registry reference.
    pub module_source: String,
    pub memory_limit_mb: u64,
    pub cpu_time_limit_secs: u64,
    pub fuel_limit: Option<u64>,
    pub capabilities: Vec<String>,
    pub environment: HashMap<String, String>,
    pub replicas: u32,
    pub auto_restart: bool,
}

/// Capability policy resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPolicyResource {
    pub name: String,
    pub description: String,
    pub rules: Vec<PolicyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub capability: String,
    pub action: PolicyAction,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    Allow,
    Deny,
    AuditOnly,
}

/// Pool resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolResource {
    pub name: String,
    pub min_size: u32,
    pub max_size: u32,
    pub module_source: String,
    pub warm_count: u32,
    pub scaling_policy: ScalingPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScalingPolicy {
    Manual,
    CpuBased { target_percent: f64 },
    QueueBased { target_queue_depth: u32 },
    ScheduleBased { cron: String },
}

/// Schema description for a resource type.
pub struct ResourceSchema {
    pub resource_type: ResourceType,
    pub description: String,
    pub properties: Vec<PropertySchema>,
    pub required: Vec<String>,
}

pub struct PropertySchema {
    pub name: String,
    pub property_type: PropertyType,
    pub description: String,
    pub default_value: Option<serde_json::Value>,
    pub validation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyType {
    String,
    Integer,
    Boolean,
    Float,
    Map,
    List,
    Object,
}

/// Validation error for a resource.
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

/// Returns the schema for the given resource type.
pub fn get_schema(resource_type: ResourceType) -> ResourceSchema {
    match resource_type {
        ResourceType::Sandbox => ResourceSchema {
            resource_type: ResourceType::Sandbox,
            description: "A sandbox execution environment".into(),
            properties: vec![
                PropertySchema {
                    name: "name".into(),
                    property_type: PropertyType::String,
                    description: "Sandbox name".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "module_source".into(),
                    property_type: PropertyType::String,
                    description: "WASM module path or registry reference".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "memory_limit_mb".into(),
                    property_type: PropertyType::Integer,
                    description: "Memory limit in megabytes".into(),
                    default_value: Some(serde_json::json!(128)),
                    validation: Some("positive integer".into()),
                },
                PropertySchema {
                    name: "cpu_time_limit_secs".into(),
                    property_type: PropertyType::Integer,
                    description: "CPU time limit in seconds".into(),
                    default_value: Some(serde_json::json!(30)),
                    validation: Some("positive integer".into()),
                },
                PropertySchema {
                    name: "replicas".into(),
                    property_type: PropertyType::Integer,
                    description: "Number of replicas".into(),
                    default_value: Some(serde_json::json!(1)),
                    validation: Some("positive integer".into()),
                },
            ],
            required: vec!["name".into(), "module_source".into()],
        },
        ResourceType::CapabilityPolicy => ResourceSchema {
            resource_type: ResourceType::CapabilityPolicy,
            description: "A capability policy defining allowed operations".into(),
            properties: vec![
                PropertySchema {
                    name: "name".into(),
                    property_type: PropertyType::String,
                    description: "Policy name".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "description".into(),
                    property_type: PropertyType::String,
                    description: "Policy description".into(),
                    default_value: Some(serde_json::json!("")),
                    validation: None,
                },
                PropertySchema {
                    name: "rules".into(),
                    property_type: PropertyType::List,
                    description: "Policy rules".into(),
                    default_value: None,
                    validation: None,
                },
            ],
            required: vec!["name".into(), "rules".into()],
        },
        ResourceType::SandboxPool => ResourceSchema {
            resource_type: ResourceType::SandboxPool,
            description: "A pool of pre-warmed sandboxes".into(),
            properties: vec![
                PropertySchema {
                    name: "name".into(),
                    property_type: PropertyType::String,
                    description: "Pool name".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "min_size".into(),
                    property_type: PropertyType::Integer,
                    description: "Minimum pool size".into(),
                    default_value: Some(serde_json::json!(0)),
                    validation: Some("non-negative integer".into()),
                },
                PropertySchema {
                    name: "max_size".into(),
                    property_type: PropertyType::Integer,
                    description: "Maximum pool size".into(),
                    default_value: Some(serde_json::json!(10)),
                    validation: Some("positive integer".into()),
                },
                PropertySchema {
                    name: "module_source".into(),
                    property_type: PropertyType::String,
                    description: "WASM module for pool sandboxes".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
            ],
            required: vec!["name".into(), "module_source".into()],
        },
        ResourceType::Module => ResourceSchema {
            resource_type: ResourceType::Module,
            description: "A WASM module registration".into(),
            properties: vec![
                PropertySchema {
                    name: "name".into(),
                    property_type: PropertyType::String,
                    description: "Module name".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "source".into(),
                    property_type: PropertyType::String,
                    description: "Module source path or URL".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
            ],
            required: vec!["name".into(), "source".into()],
        },
        ResourceType::SecretStore => ResourceSchema {
            resource_type: ResourceType::SecretStore,
            description: "A secret store for sensitive configuration".into(),
            properties: vec![
                PropertySchema {
                    name: "name".into(),
                    property_type: PropertyType::String,
                    description: "Store name".into(),
                    default_value: None,
                    validation: Some("non-empty string".into()),
                },
                PropertySchema {
                    name: "backend".into(),
                    property_type: PropertyType::String,
                    description: "Storage backend type".into(),
                    default_value: Some(serde_json::json!("memory")),
                    validation: None,
                },
            ],
            required: vec!["name".into()],
        },
    }
}

/// Validates a resource against its schema.
pub fn validate_resource(resource: &IacResource) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if resource.name.is_empty() {
        errors.push(ValidationError {
            field: "name".into(),
            message: "Resource name cannot be empty".into(),
        });
    }

    let schema = get_schema(resource.resource_type.clone());

    // Check required properties
    for required in &schema.required {
        if required == "name" {
            continue; // already checked above
        }
        if !resource.properties.contains_key(required) {
            errors.push(ValidationError {
                field: required.clone(),
                message: format!("Required property '{}' is missing", required),
            });
        }
    }

    // Validate property types
    for (key, value) in &resource.properties {
        if let Some(prop_schema) = schema.properties.iter().find(|p| &p.name == key) {
            let type_ok = match prop_schema.property_type {
                PropertyType::String => value.is_string(),
                PropertyType::Integer => value.is_i64() || value.is_u64(),
                PropertyType::Boolean => value.is_boolean(),
                PropertyType::Float => value.is_f64(),
                PropertyType::Map => value.is_object(),
                PropertyType::List => value.is_array(),
                PropertyType::Object => value.is_object(),
            };
            if !type_ok {
                errors.push(ValidationError {
                    field: key.clone(),
                    message: format!(
                        "Property '{}' has wrong type, expected {:?}",
                        key, prop_schema.property_type
                    ),
                });
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sandbox_resource() -> IacResource {
        let mut properties = HashMap::new();
        properties.insert("module_source".into(), serde_json::json!("registry/hello.wasm"));
        properties.insert("memory_limit_mb".into(), serde_json::json!(128));
        IacResource {
            resource_type: ResourceType::Sandbox,
            name: "test-sandbox".into(),
            id: None,
            properties,
            labels: HashMap::new(),
            depends_on: Vec::new(),
            lifecycle: LifecyclePolicy::default(),
        }
    }

    #[test]
    fn test_resource_type_display() {
        assert_eq!(ResourceType::Sandbox.to_string(), "sandbox");
        assert_eq!(ResourceType::CapabilityPolicy.to_string(), "capability_policy");
        assert_eq!(ResourceType::SandboxPool.to_string(), "sandbox_pool");
        assert_eq!(ResourceType::Module.to_string(), "module");
        assert_eq!(ResourceType::SecretStore.to_string(), "secret_store");
    }

    #[test]
    fn test_resource_type_equality() {
        assert_eq!(ResourceType::Sandbox, ResourceType::Sandbox);
        assert_ne!(ResourceType::Sandbox, ResourceType::Module);
    }

    #[test]
    fn test_resource_type_serialization() {
        let rt = ResourceType::Sandbox;
        let json = serde_json::to_string(&rt).unwrap();
        let deserialized: ResourceType = serde_json::from_str(&json).unwrap();
        assert_eq!(rt, deserialized);
    }

    #[test]
    fn test_iac_resource_serialization() {
        let resource = make_sandbox_resource();
        let json = serde_json::to_string(&resource).unwrap();
        let deserialized: IacResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test-sandbox");
        assert_eq!(deserialized.resource_type, ResourceType::Sandbox);
    }

    #[test]
    fn test_validate_resource_valid() {
        let resource = make_sandbox_resource();
        let errors = validate_resource(&resource);
        assert!(
            errors.is_empty(),
            "Expected no errors: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_validate_resource_empty_name() {
        let mut resource = make_sandbox_resource();
        resource.name = String::new();
        let errors = validate_resource(&resource);
        assert!(errors.iter().any(|e| e.field == "name"));
    }

    #[test]
    fn test_validate_resource_missing_required() {
        let resource = IacResource {
            resource_type: ResourceType::Sandbox,
            name: "test".into(),
            id: None,
            properties: HashMap::new(),
            labels: HashMap::new(),
            depends_on: Vec::new(),
            lifecycle: LifecyclePolicy::default(),
        };
        let errors = validate_resource(&resource);
        assert!(errors.iter().any(|e| e.field == "module_source"));
    }

    #[test]
    fn test_validate_resource_wrong_type() {
        let mut resource = make_sandbox_resource();
        resource.properties.insert("memory_limit_mb".into(), serde_json::json!("not a number"));
        let errors = validate_resource(&resource);
        assert!(errors.iter().any(|e| e.field == "memory_limit_mb"));
    }

    #[test]
    fn test_get_schema_all_types() {
        let types = vec![
            ResourceType::Sandbox,
            ResourceType::CapabilityPolicy,
            ResourceType::SandboxPool,
            ResourceType::Module,
            ResourceType::SecretStore,
        ];
        for rt in types {
            let schema = get_schema(rt.clone());
            assert_eq!(schema.resource_type, rt);
            assert!(!schema.description.is_empty());
            assert!(!schema.properties.is_empty());
        }
    }

    #[test]
    fn test_lifecycle_policy_default() {
        let policy = LifecyclePolicy::default();
        assert!(!policy.prevent_destroy);
        assert!(!policy.create_before_destroy);
        assert!(policy.ignore_changes.is_empty());
    }

    #[test]
    fn test_policy_action_serialization() {
        let actions = vec![PolicyAction::Allow, PolicyAction::Deny, PolicyAction::AuditOnly];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let deserialized: PolicyAction = serde_json::from_str(&json).unwrap();
            assert_eq!(action, deserialized);
        }
    }

    #[test]
    fn test_sandbox_resource_serialization() {
        let sr = SandboxResource {
            name: "my-sandbox".into(),
            module_source: "hello.wasm".into(),
            memory_limit_mb: 256,
            cpu_time_limit_secs: 60,
            fuel_limit: Some(1_000_000),
            capabilities: vec!["stdout".into()],
            environment: HashMap::new(),
            replicas: 2,
            auto_restart: true,
        };
        let json = serde_json::to_string(&sr).unwrap();
        let deserialized: SandboxResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "my-sandbox");
        assert_eq!(deserialized.replicas, 2);
        assert!(deserialized.auto_restart);
    }

    #[test]
    fn test_pool_resource_scaling_policy() {
        let pool = PoolResource {
            name: "pool-1".into(),
            min_size: 2,
            max_size: 10,
            module_source: "module.wasm".into(),
            warm_count: 3,
            scaling_policy: ScalingPolicy::CpuBased { target_percent: 75.0 },
        };
        let json = serde_json::to_string(&pool).unwrap();
        let deserialized: PoolResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "pool-1");
        assert_eq!(deserialized.min_size, 2);
    }
}
