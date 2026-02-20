//! Multi-cloud deployment configuration types and deployment planner.
//!
//! This module provides types for describing deployment targets across cloud
//! providers, generating deployment plans, and producing deployment manifests.
//! It does **not** actually deploy — it produces configurations that deployment
//! tools can consume.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// CloudProvider
// ---------------------------------------------------------------------------

/// Supported cloud providers for WASM sandbox deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloudProvider {
    Aws,
    Gcp,
    Azure,
    Cloudflare,
    Fly,
}

impl fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aws => write!(f, "AWS"),
            Self::Gcp => write!(f, "GCP"),
            Self::Azure => write!(f, "Azure"),
            Self::Cloudflare => write!(f, "Cloudflare"),
            Self::Fly => write!(f, "Fly"),
        }
    }
}

impl FromStr for CloudProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "aws" => Ok(Self::Aws),
            "gcp" => Ok(Self::Gcp),
            "azure" => Ok(Self::Azure),
            "cloudflare" => Ok(Self::Cloudflare),
            "fly" => Ok(Self::Fly),
            other => Err(format!("unknown cloud provider: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// DeploymentTarget
// ---------------------------------------------------------------------------

/// A single deployment target (provider + region + service configuration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentTarget {
    pub provider: CloudProvider,
    pub region: String,
    pub service_name: String,
    pub instance_type: Option<String>,
    pub replicas: u32,
    pub env_vars: HashMap<String, String>,
    pub labels: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// DeploymentTargetBuilder
// ---------------------------------------------------------------------------

/// Builder for [`DeploymentTarget`].
pub struct DeploymentTargetBuilder {
    provider: CloudProvider,
    region: String,
    service_name: String,
    instance_type: Option<String>,
    replicas: u32,
    env_vars: HashMap<String, String>,
    labels: HashMap<String, String>,
}

impl DeploymentTargetBuilder {
    pub fn new(provider: CloudProvider, region: impl Into<String>) -> Self {
        Self {
            provider,
            region: region.into(),
            service_name: String::new(),
            instance_type: None,
            replicas: 1,
            env_vars: HashMap::new(),
            labels: HashMap::new(),
        }
    }

    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    pub fn instance_type(mut self, t: impl Into<String>) -> Self {
        self.instance_type = Some(t.into());
        self
    }

    pub fn replicas(mut self, n: u32) -> Self {
        self.replicas = n;
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> DeploymentTarget {
        DeploymentTarget {
            provider: self.provider,
            region: self.region,
            service_name: self.service_name,
            instance_type: self.instance_type,
            replicas: self.replicas,
            env_vars: self.env_vars,
            labels: self.labels,
        }
    }
}

// ---------------------------------------------------------------------------
// HealthCheck
// ---------------------------------------------------------------------------

/// Health-check configuration for a deployed service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub path: String,
    pub interval_seconds: u32,
    pub timeout_seconds: u32,
    pub healthy_threshold: u32,
    pub unhealthy_threshold: u32,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            path: "/health".to_string(),
            interval_seconds: 30,
            timeout_seconds: 5,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// ScalingConfig
// ---------------------------------------------------------------------------

/// Auto-scaling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingConfig {
    pub min_instances: u32,
    pub max_instances: u32,
    pub target_cpu_percent: u32,
    pub scale_down_cooldown_seconds: u32,
}

// ---------------------------------------------------------------------------
// DeploymentConfig
// ---------------------------------------------------------------------------

/// Top-level deployment configuration describing what to deploy and where.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub name: String,
    pub version: String,
    pub wasm_path: String,
    pub targets: Vec<DeploymentTarget>,
    pub health_check: Option<HealthCheck>,
    pub scaling: Option<ScalingConfig>,
}

// ---------------------------------------------------------------------------
// DeployAction / DeploymentStep / CostEstimate / CostItem / DeploymentPlan
// ---------------------------------------------------------------------------

/// Actions that a deployment step can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeployAction {
    CreateService,
    UpdateService,
    ScaleService,
    ConfigureHealthCheck,
    SetEnvironment,
    DestroyService,
}

impl fmt::Display for DeployAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateService => write!(f, "CreateService"),
            Self::UpdateService => write!(f, "UpdateService"),
            Self::ScaleService => write!(f, "ScaleService"),
            Self::ConfigureHealthCheck => write!(f, "ConfigureHealthCheck"),
            Self::SetEnvironment => write!(f, "SetEnvironment"),
            Self::DestroyService => write!(f, "DestroyService"),
        }
    }
}

/// A single step in a deployment plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStep {
    pub order: u32,
    pub action: DeployAction,
    pub target: DeploymentTarget,
    pub description: String,
}

/// Itemised cost entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostItem {
    pub provider: CloudProvider,
    pub region: String,
    pub item: String,
    pub monthly_usd: f64,
}

/// Rough cost estimate for a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub monthly_usd: f64,
    pub breakdown: Vec<CostItem>,
}

/// Generated deployment plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub config_name: String,
    pub steps: Vec<DeploymentStep>,
    pub estimated_cost: Option<CostEstimate>,
}

// ---------------------------------------------------------------------------
// DeploymentPlanner
// ---------------------------------------------------------------------------

/// Generates deployment plans, manifests, and validates configurations.
pub struct DeploymentPlanner;

impl DeploymentPlanner {
    /// Generate a deployment plan from the given configuration.
    pub fn plan(config: &DeploymentConfig) -> DeploymentPlan {
        let mut steps = Vec::new();
        let mut order: u32 = 1;

        for target in &config.targets {
            steps.push(DeploymentStep {
                order,
                action: DeployAction::CreateService,
                target: target.clone(),
                description: format!(
                    "Create service {} on {} in {}",
                    target.service_name, target.provider, target.region
                ),
            });
            order += 1;

            if config.health_check.is_some() {
                steps.push(DeploymentStep {
                    order,
                    action: DeployAction::ConfigureHealthCheck,
                    target: target.clone(),
                    description: format!(
                        "Configure health check for {} on {}",
                        target.service_name, target.provider
                    ),
                });
                order += 1;
            }

            if !target.env_vars.is_empty() {
                steps.push(DeploymentStep {
                    order,
                    action: DeployAction::SetEnvironment,
                    target: target.clone(),
                    description: format!(
                        "Set environment variables for {} on {}",
                        target.service_name, target.provider
                    ),
                });
                order += 1;
            }

            if config.scaling.is_some() {
                steps.push(DeploymentStep {
                    order,
                    action: DeployAction::ScaleService,
                    target: target.clone(),
                    description: format!(
                        "Configure auto-scaling for {} on {}",
                        target.service_name, target.provider
                    ),
                });
                order += 1;
            }
        }

        let estimated_cost = Some(Self::estimate_cost(config));

        DeploymentPlan {
            config_name: config.name.clone(),
            steps,
            estimated_cost,
        }
    }

    /// Validate a deployment configuration, returning any errors found.
    pub fn validate(config: &DeploymentConfig) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if config.name.is_empty() {
            errors.push("Deployment name must not be empty".to_string());
        }

        if config.targets.is_empty() {
            errors.push("At least one deployment target is required".to_string());
        }

        for (i, target) in config.targets.iter().enumerate() {
            if target.region.is_empty() {
                errors.push(format!("Target {i}: region must not be empty"));
            }
            if target.service_name.is_empty() {
                errors.push(format!("Target {i}: service_name must not be empty"));
            }
            if target.replicas == 0 {
                errors.push(format!("Target {i}: replicas must be greater than 0"));
            }
        }

        if let Some(ref scaling) = config.scaling {
            if scaling.min_instances > scaling.max_instances {
                errors.push(
                    "Scaling min_instances must be <= max_instances".to_string(),
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Generate a provider-specific manifest string.
    pub fn generate_manifest(config: &DeploymentConfig, provider: CloudProvider) -> String {
        match provider {
            CloudProvider::Aws => Self::aws_manifest(config),
            CloudProvider::Gcp => Self::gcp_manifest(config),
            CloudProvider::Azure => Self::azure_manifest(config),
            CloudProvider::Cloudflare => Self::cloudflare_manifest(config),
            CloudProvider::Fly => Self::fly_manifest(config),
        }
    }

    // -- private helpers ----------------------------------------------------

    fn per_replica_cost(provider: CloudProvider) -> f64 {
        match provider {
            CloudProvider::Aws => 5.0,
            CloudProvider::Gcp => 4.5,
            CloudProvider::Azure => 5.0,
            CloudProvider::Cloudflare => 0.5,
            CloudProvider::Fly => 2.0,
        }
    }

    fn estimate_cost(config: &DeploymentConfig) -> CostEstimate {
        let mut breakdown = Vec::new();
        let mut total = 0.0;

        for target in &config.targets {
            let cost = Self::per_replica_cost(target.provider) * target.replicas as f64;
            total += cost;
            breakdown.push(CostItem {
                provider: target.provider,
                region: target.region.clone(),
                item: format!("{} x {} replicas", target.service_name, target.replicas),
                monthly_usd: cost,
            });
        }

        CostEstimate {
            monthly_usd: total,
            breakdown,
        }
    }

    fn aws_manifest(config: &DeploymentConfig) -> String {
        let targets: Vec<&DeploymentTarget> = config
            .targets
            .iter()
            .filter(|t| t.provider == CloudProvider::Aws)
            .collect();
        let target = targets.first().cloned().unwrap_or_else(|| &config.targets[0]);

        let env_json: String = target
            .env_vars
            .iter()
            .map(|(k, v)| format!("      {{\"name\": \"{k}\", \"value\": \"{v}\"}}"))
            .collect::<Vec<_>>()
            .join(",\n");

        format!(
            r#"{{
  "family": "{name}",
  "containerDefinitions": [
    {{
      "name": "{service}",
      "image": "{wasm}",
      "cpu": 256,
      "memory": 512,
      "essential": true,
      "environment": [
{env}
      ]
    }}
  ],
  "requiresCompatibilities": ["FARGATE"],
  "networkMode": "awsvpc",
  "cpu": "256",
  "memory": "512"
}}"#,
            name = config.name,
            service = target.service_name,
            wasm = config.wasm_path,
            env = env_json,
        )
    }

    fn gcp_manifest(config: &DeploymentConfig) -> String {
        let targets: Vec<&DeploymentTarget> = config
            .targets
            .iter()
            .filter(|t| t.provider == CloudProvider::Gcp)
            .collect();
        let target = targets.first().cloned().unwrap_or_else(|| &config.targets[0]);

        let env_yaml: String = target
            .env_vars
            .iter()
            .map(|(k, v)| format!("        - name: {k}\n          value: \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"apiVersion: serving.knative.dev/v1
kind: Service
metadata:
  name: {service}
spec:
  template:
    spec:
      containers:
        - image: {wasm}
          env:
{env}
"#,
            service = target.service_name,
            wasm = config.wasm_path,
            env = env_yaml,
        )
    }

    fn azure_manifest(config: &DeploymentConfig) -> String {
        let targets: Vec<&DeploymentTarget> = config
            .targets
            .iter()
            .filter(|t| t.provider == CloudProvider::Azure)
            .collect();
        let target = targets.first().cloned().unwrap_or_else(|| &config.targets[0]);

        let env_yaml: String = target
            .env_vars
            .iter()
            .map(|(k, v)| format!("      - name: {k}\n        value: \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"apiVersion: apps/v1
kind: ContainerApp
metadata:
  name: {service}
spec:
  replicas: {replicas}
  containers:
    - image: {wasm}
      name: {service}
      env:
{env}
"#,
            service = target.service_name,
            wasm = config.wasm_path,
            replicas = target.replicas,
            env = env_yaml,
        )
    }

    fn cloudflare_manifest(config: &DeploymentConfig) -> String {
        let targets: Vec<&DeploymentTarget> = config
            .targets
            .iter()
            .filter(|t| t.provider == CloudProvider::Cloudflare)
            .collect();
        let target = targets.first().cloned().unwrap_or_else(|| &config.targets[0]);

        let vars: String = target
            .env_vars
            .iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"name = "{service}"
main = "{wasm}"
compatibility_date = "2024-01-01"

[vars]
{vars}
"#,
            service = target.service_name,
            wasm = config.wasm_path,
            vars = vars,
        )
    }

    fn fly_manifest(config: &DeploymentConfig) -> String {
        let targets: Vec<&DeploymentTarget> = config
            .targets
            .iter()
            .filter(|t| t.provider == CloudProvider::Fly)
            .collect();
        let target = targets.first().cloned().unwrap_or_else(|| &config.targets[0]);

        let env_lines: String = target
            .env_vars
            .iter()
            .map(|(k, v)| format!("  {k} = \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"app = "{service}"
primary_region = "{region}"

[build]
  image = "{wasm}"

[env]
{env}

[[services]]
  internal_port = 8080
  protocol = "tcp"
"#,
            service = target.service_name,
            region = target.region,
            wasm = config.wasm_path,
            env = env_lines,
        )
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- CloudProvider Display & FromStr ------------------------------------

    #[test]
    fn test_cloud_provider_display() {
        assert_eq!(CloudProvider::Aws.to_string(), "AWS");
        assert_eq!(CloudProvider::Gcp.to_string(), "GCP");
        assert_eq!(CloudProvider::Azure.to_string(), "Azure");
        assert_eq!(CloudProvider::Cloudflare.to_string(), "Cloudflare");
        assert_eq!(CloudProvider::Fly.to_string(), "Fly");
    }

    #[test]
    fn test_cloud_provider_from_str() {
        assert_eq!("aws".parse::<CloudProvider>().unwrap(), CloudProvider::Aws);
        assert_eq!("GCP".parse::<CloudProvider>().unwrap(), CloudProvider::Gcp);
        assert_eq!(
            "Azure".parse::<CloudProvider>().unwrap(),
            CloudProvider::Azure
        );
        assert_eq!(
            "CLOUDFLARE".parse::<CloudProvider>().unwrap(),
            CloudProvider::Cloudflare
        );
        assert_eq!("fly".parse::<CloudProvider>().unwrap(), CloudProvider::Fly);
        assert!("unknown".parse::<CloudProvider>().is_err());
    }

    // -- DeploymentTargetBuilder -------------------------------------------

    #[test]
    fn test_deployment_target_builder() {
        let target = DeploymentTargetBuilder::new(CloudProvider::Aws, "us-east-1")
            .service_name("my-service")
            .instance_type("t3.micro")
            .replicas(3)
            .env("DATABASE_URL", "postgres://localhost/db")
            .label("team", "platform")
            .build();

        assert_eq!(target.provider, CloudProvider::Aws);
        assert_eq!(target.region, "us-east-1");
        assert_eq!(target.service_name, "my-service");
        assert_eq!(target.instance_type.as_deref(), Some("t3.micro"));
        assert_eq!(target.replicas, 3);
        assert_eq!(target.env_vars.get("DATABASE_URL").unwrap(), "postgres://localhost/db");
        assert_eq!(target.labels.get("team").unwrap(), "platform");
    }

    // -- DeploymentConfig creation -----------------------------------------

    #[test]
    fn test_deployment_config_creation() {
        let config = make_config();
        assert_eq!(config.name, "test-app");
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.targets.len(), 1);
    }

    // -- HealthCheck default -----------------------------------------------

    #[test]
    fn test_health_check_default() {
        let hc = HealthCheck::default();
        assert_eq!(hc.path, "/health");
        assert_eq!(hc.interval_seconds, 30);
        assert_eq!(hc.timeout_seconds, 5);
        assert_eq!(hc.healthy_threshold, 2);
        assert_eq!(hc.unhealthy_threshold, 3);
    }

    // -- DeploymentPlanner plan generation ----------------------------------

    #[test]
    fn test_planner_plan_generation() {
        let config = make_config();
        let plan = DeploymentPlanner::plan(&config);
        assert_eq!(plan.config_name, "test-app");
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn test_plan_step_count() {
        let config = make_config_full();
        let plan = DeploymentPlanner::plan(&config);
        // 1 target => CreateService + ConfigureHealthCheck + SetEnvironment + ScaleService = 4
        assert_eq!(plan.steps.len(), 4);
    }

    #[test]
    fn test_cost_estimate_calculation() {
        let config = make_config();
        let plan = DeploymentPlanner::plan(&config);
        let cost = plan.estimated_cost.as_ref().unwrap();
        // AWS, 2 replicas => 2 * $5 = $10
        assert!((cost.monthly_usd - 10.0).abs() < f64::EPSILON);
    }

    // -- validate -----------------------------------------------------------

    #[test]
    fn test_validate_valid_config() {
        let config = make_config();
        assert!(DeploymentPlanner::validate(&config).is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let mut config = make_config();
        config.name = String::new();
        let errs = DeploymentPlanner::validate(&config).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("name")));
    }

    #[test]
    fn test_validate_no_targets() {
        let mut config = make_config();
        config.targets.clear();
        let errs = DeploymentPlanner::validate(&config).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("target")));
    }

    #[test]
    fn test_validate_zero_replicas() {
        let mut config = make_config();
        config.targets[0].replicas = 0;
        let errs = DeploymentPlanner::validate(&config).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("replicas")));
    }

    #[test]
    fn test_validate_bad_scaling() {
        let mut config = make_config();
        config.scaling = Some(ScalingConfig {
            min_instances: 10,
            max_instances: 2,
            target_cpu_percent: 70,
            scale_down_cooldown_seconds: 60,
        });
        let errs = DeploymentPlanner::validate(&config).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("min_instances")));
    }

    // -- generate_manifest --------------------------------------------------

    #[test]
    fn test_generate_manifest_aws() {
        let config = make_config();
        let manifest = DeploymentPlanner::generate_manifest(&config, CloudProvider::Aws);
        assert!(manifest.contains("containerDefinitions"));
        assert!(manifest.contains("test-app"));
        assert!(manifest.contains("FARGATE"));
    }

    #[test]
    fn test_generate_manifest_cloudflare() {
        let target = DeploymentTargetBuilder::new(CloudProvider::Cloudflare, "global")
            .service_name("edge-worker")
            .replicas(1)
            .env("API_KEY", "secret")
            .build();

        let config = DeploymentConfig {
            name: "edge-app".to_string(),
            version: "1.0.0".to_string(),
            wasm_path: "app.wasm".to_string(),
            targets: vec![target],
            health_check: None,
            scaling: None,
        };

        let manifest =
            DeploymentPlanner::generate_manifest(&config, CloudProvider::Cloudflare);
        assert!(manifest.contains("compatibility_date"));
        assert!(manifest.contains("edge-worker"));
        assert!(manifest.contains("app.wasm"));
    }

    #[test]
    fn test_generate_manifest_fly() {
        let target = DeploymentTargetBuilder::new(CloudProvider::Fly, "ord")
            .service_name("fly-app")
            .replicas(1)
            .env("PORT", "8080")
            .build();

        let config = DeploymentConfig {
            name: "fly-deploy".to_string(),
            version: "0.1.0".to_string(),
            wasm_path: "module.wasm".to_string(),
            targets: vec![target],
            health_check: None,
            scaling: None,
        };

        let manifest = DeploymentPlanner::generate_manifest(&config, CloudProvider::Fly);
        assert!(manifest.contains("app = \"fly-app\""));
        assert!(manifest.contains("primary_region = \"ord\""));
        assert!(manifest.contains("module.wasm"));
    }

    // -- DeployAction Display -----------------------------------------------

    #[test]
    fn test_deploy_action_display() {
        assert_eq!(DeployAction::CreateService.to_string(), "CreateService");
        assert_eq!(DeployAction::UpdateService.to_string(), "UpdateService");
        assert_eq!(DeployAction::ScaleService.to_string(), "ScaleService");
        assert_eq!(
            DeployAction::ConfigureHealthCheck.to_string(),
            "ConfigureHealthCheck"
        );
        assert_eq!(DeployAction::SetEnvironment.to_string(), "SetEnvironment");
        assert_eq!(DeployAction::DestroyService.to_string(), "DestroyService");
    }

    // -- CostEstimate breakdown ---------------------------------------------

    #[test]
    fn test_cost_estimate_breakdown() {
        let config = make_config();
        let plan = DeploymentPlanner::plan(&config);
        let cost = plan.estimated_cost.as_ref().unwrap();
        assert_eq!(cost.breakdown.len(), 1);
        assert_eq!(cost.breakdown[0].provider, CloudProvider::Aws);
        assert!((cost.breakdown[0].monthly_usd - 10.0).abs() < f64::EPSILON);
    }

    // -- Multiple targets plan ----------------------------------------------

    #[test]
    fn test_multiple_targets_plan() {
        let aws = DeploymentTargetBuilder::new(CloudProvider::Aws, "us-east-1")
            .service_name("svc-aws")
            .replicas(2)
            .build();
        let gcp = DeploymentTargetBuilder::new(CloudProvider::Gcp, "us-central1")
            .service_name("svc-gcp")
            .replicas(3)
            .build();

        let config = DeploymentConfig {
            name: "multi".to_string(),
            version: "1.0.0".to_string(),
            wasm_path: "app.wasm".to_string(),
            targets: vec![aws, gcp],
            health_check: None,
            scaling: None,
        };

        let plan = DeploymentPlanner::plan(&config);
        // 2 targets, no health check / env / scaling => 2 CreateService steps
        assert_eq!(plan.steps.len(), 2);

        let cost = plan.estimated_cost.as_ref().unwrap();
        // AWS: 2*5=10, GCP: 3*4.5=13.5 => 23.5
        assert!((cost.monthly_usd - 23.5).abs() < f64::EPSILON);
        assert_eq!(cost.breakdown.len(), 2);
    }

    // -- helpers ------------------------------------------------------------

    fn make_config() -> DeploymentConfig {
        let target = DeploymentTargetBuilder::new(CloudProvider::Aws, "us-east-1")
            .service_name("test-service")
            .replicas(2)
            .build();

        DeploymentConfig {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            wasm_path: "module.wasm".to_string(),
            targets: vec![target],
            health_check: None,
            scaling: None,
        }
    }

    fn make_config_full() -> DeploymentConfig {
        let target = DeploymentTargetBuilder::new(CloudProvider::Aws, "us-east-1")
            .service_name("full-service")
            .replicas(2)
            .env("APP_ENV", "production")
            .build();

        DeploymentConfig {
            name: "full-app".to_string(),
            version: "2.0.0".to_string(),
            wasm_path: "module.wasm".to_string(),
            targets: vec![target],
            health_check: Some(HealthCheck::default()),
            scaling: Some(ScalingConfig {
                min_instances: 1,
                max_instances: 10,
                target_cpu_percent: 70,
                scale_down_cooldown_seconds: 300,
            }),
        }
    }
}
