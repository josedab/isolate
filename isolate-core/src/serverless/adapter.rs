use super::function::{ServerlessFunction, Trigger};
use serde::{Deserialize, Serialize};

/// Supported serverless frameworks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Framework {
    OpenFaaS,
    Knative,
    Fission,
    AwsSam,
}

impl std::fmt::Display for Framework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Framework::OpenFaaS => write!(f, "OpenFaaS"),
            Framework::Knative => write!(f, "Knative"),
            Framework::Fission => write!(f, "Fission"),
            Framework::AwsSam => write!(f, "AWS SAM"),
        }
    }
}

/// Generated deployment manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentManifest {
    pub framework: Framework,
    pub content: String,
    pub format: ManifestFormat,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestFormat {
    Yaml,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub content: String,
}

/// Framework adapter trait.
pub trait FrameworkAdapter {
    fn framework(&self) -> Framework;
    fn generate_manifest(&self, function: &ServerlessFunction) -> DeploymentManifest;
    fn generate_dockerfile(&self, function: &ServerlessFunction) -> String;
    fn validate(&self, function: &ServerlessFunction) -> Vec<ValidationIssue>;
}

pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
    pub field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

/// OpenFaaS adapter.
pub struct OpenFaaSAdapter;

impl FrameworkAdapter for OpenFaaSAdapter {
    fn framework(&self) -> Framework {
        Framework::OpenFaaS
    }

    fn generate_manifest(&self, function: &ServerlessFunction) -> DeploymentManifest {
        let env_lines: String = function
            .environment
            .iter()
            .map(|(k, v)| format!("      {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");

        let env_section = if env_lines.is_empty() {
            String::new()
        } else {
            format!("    environment:\n{env_lines}\n")
        };

        let labels_lines: String = function
            .labels
            .iter()
            .map(|(k, v)| format!("      {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");

        let labels_section = if labels_lines.is_empty() {
            String::new()
        } else {
            format!("    labels:\n{labels_lines}\n")
        };

        let content = format!(
            "version: 1.0\nprovider:\n  name: openfaas\nfunctions:\n  {name}:\n    lang: wasm\n    handler: ./{name}\n    image: {name}:latest\n{env}{labels}    limits:\n      memory: {mem}Mi\n      timeout: {timeout}s\n    scaling:\n      min: {min}\n      max: {max}\n",
            name = function.name,
            env = env_section,
            labels = labels_section,
            mem = function.runtime.memory_mb,
            timeout = function.runtime.timeout.as_secs(),
            min = function.scaling.min_instances,
            max = function.scaling.max_instances,
        );

        DeploymentManifest {
            framework: Framework::OpenFaaS,
            content,
            format: ManifestFormat::Yaml,
            files: vec![],
        }
    }

    fn generate_dockerfile(&self, function: &ServerlessFunction) -> String {
        format!(
            "FROM openfaas/of-watchdog:latest AS watchdog\nFROM isolate-runtime:latest\nCOPY --from=watchdog /fwatchdog /usr/bin/fwatchdog\nCOPY {name}.wasm /home/app/function.wasm\nENV fprocess=\"isolate-run /home/app/function.wasm\"\nENV read_timeout=\"{timeout}s\"\nENV write_timeout=\"{timeout}s\"\nHEALTHCHECK --interval=3s CMD [ -e /tmp/.lock ] || exit 1\nCMD [\"fwatchdog\"]\n",
            name = function.name,
            timeout = function.runtime.timeout.as_secs(),
        )
    }

    fn validate(&self, function: &ServerlessFunction) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if function.name.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "Function name is required".to_string(),
                field: Some("name".to_string()),
            });
        }
        if function.runtime.memory_mb > 4096 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "Memory exceeds typical OpenFaaS limits".to_string(),
                field: Some("runtime.memory_mb".to_string()),
            });
        }
        issues
    }
}

/// Knative adapter.
pub struct KnativeAdapter;

impl FrameworkAdapter for KnativeAdapter {
    fn framework(&self) -> Framework {
        Framework::Knative
    }

    fn generate_manifest(&self, function: &ServerlessFunction) -> DeploymentManifest {
        let env_lines: String = function
            .environment
            .iter()
            .map(|(k, v)| format!("            - name: {k}\n              value: \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n");

        let env_section = if env_lines.is_empty() {
            String::new()
        } else {
            format!("          env:\n{env_lines}\n")
        };

        let content = format!(
            "apiVersion: serving.knative.dev/v1\nkind: Service\nmetadata:\n  name: {name}\nspec:\n  template:\n    metadata:\n      annotations:\n        autoscaling.knative.dev/minScale: \"{min}\"\n        autoscaling.knative.dev/maxScale: \"{max}\"\n        autoscaling.knative.dev/target: \"{target}\"\n    spec:\n      containerConcurrency: {concurrency}\n      timeoutSeconds: {timeout}\n      containers:\n        - image: {name}:latest\n          resources:\n            limits:\n              memory: {mem}Mi\n{env}",
            name = function.name,
            min = function.scaling.min_instances,
            max = function.scaling.max_instances,
            target = function.scaling.target_concurrency,
            concurrency = function.runtime.concurrency,
            timeout = function.runtime.timeout.as_secs(),
            mem = function.runtime.memory_mb,
            env = env_section,
        );

        DeploymentManifest {
            framework: Framework::Knative,
            content,
            format: ManifestFormat::Yaml,
            files: vec![],
        }
    }

    fn generate_dockerfile(&self, function: &ServerlessFunction) -> String {
        format!(
            "FROM isolate-runtime:latest\nCOPY {name}.wasm /app/function.wasm\nENV PORT=8080\nENV FUNCTION_MODULE=/app/function.wasm\nEXPOSE 8080\nCMD [\"isolate-serve\", \"--port\", \"8080\", \"/app/function.wasm\"]\n",
            name = function.name,
        )
    }

    fn validate(&self, function: &ServerlessFunction) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if function.name.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "Function name is required".to_string(),
                field: Some("name".to_string()),
            });
        }
        if function.runtime.timeout.as_secs() > 600 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "Timeout exceeds typical Knative limits (600s)".to_string(),
                field: Some("runtime.timeout".to_string()),
            });
        }
        issues
    }
}

/// Fission adapter.
pub struct FissionAdapter;

impl FrameworkAdapter for FissionAdapter {
    fn framework(&self) -> Framework {
        Framework::Fission
    }

    fn generate_manifest(&self, function: &ServerlessFunction) -> DeploymentManifest {
        let http_triggers: Vec<String> = function
            .triggers
            .iter()
            .filter_map(|t| match t {
                Trigger::Http { path, methods } => {
                    let methods_str = methods
                        .iter()
                        .map(|m| m.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    Some(format!(
                        "---\napiVersion: fission.io/v1\nkind: HTTPTrigger\nmetadata:\n  name: {name}-trigger\nspec:\n  functionref:\n    name: {name}\n  relativeurl: {path}\n  methods: [{methods}]\n",
                        name = function.name,
                        path = path,
                        methods = methods_str,
                    ))
                }
                _ => None,
            })
            .collect();

        let content = format!(
            "apiVersion: fission.io/v1\nkind: Function\nmetadata:\n  name: {name}\nspec:\n  environment:\n    name: isolate\n  package:\n    name: {name}-pkg\n  resources:\n    limits:\n      memory: {mem}Mi\n{triggers}",
            name = function.name,
            mem = function.runtime.memory_mb,
            triggers = http_triggers.join(""),
        );

        DeploymentManifest {
            framework: Framework::Fission,
            content,
            format: ManifestFormat::Yaml,
            files: vec![],
        }
    }

    fn generate_dockerfile(&self, function: &ServerlessFunction) -> String {
        format!(
            "FROM fission/binary-env:latest\nCOPY {name}.wasm /userfunc/function.wasm\nCMD [\"isolate-run\", \"/userfunc/function.wasm\"]\n",
            name = function.name,
        )
    }

    fn validate(&self, function: &ServerlessFunction) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if function.name.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "Function name is required".to_string(),
                field: Some("name".to_string()),
            });
        }
        if function.name.len() > 63 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "Function name exceeds Kubernetes 63-char limit".to_string(),
                field: Some("name".to_string()),
            });
        }
        issues
    }
}

/// AWS SAM adapter.
pub struct AwsSamAdapter;

impl FrameworkAdapter for AwsSamAdapter {
    fn framework(&self) -> Framework {
        Framework::AwsSam
    }

    fn generate_manifest(&self, function: &ServerlessFunction) -> DeploymentManifest {
        let env_lines: String = function
            .environment
            .iter()
            .map(|(k, v)| format!("          {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");

        let env_section = if env_lines.is_empty() {
            String::new()
        } else {
            format!("      Environment:\n        Variables:\n{env_lines}\n")
        };

        let event_lines: String = function
            .triggers
            .iter()
            .filter_map(|t| match t {
                Trigger::Http { path, methods } => {
                    let method = methods.first().map(|m| m.to_string()).unwrap_or_default();
                    Some(format!(
                        "        ApiEvent:\n          Type: Api\n          Properties:\n            Path: {path}\n            Method: {method}\n"
                    ))
                }
                Trigger::Schedule { cron } => Some(format!(
                    "        ScheduleEvent:\n          Type: Schedule\n          Properties:\n            Schedule: cron({cron})\n"
                )),
                _ => None,
            })
            .collect();

        let events_section = if event_lines.is_empty() {
            String::new()
        } else {
            format!("      Events:\n{event_lines}")
        };

        let content = format!(
            "AWSTemplateFormatVersion: '2010-09-09'\nTransform: AWS::Serverless-2016-10-31\nResources:\n  {name}:\n    Type: AWS::Serverless::Function\n    Properties:\n      Runtime: provided.al2\n      Handler: bootstrap\n      MemorySize: {mem}\n      Timeout: {timeout}\n{env}{events}",
            name = function.name,
            mem = function.runtime.memory_mb,
            timeout = function.runtime.timeout.as_secs(),
            env = env_section,
            events = events_section,
        );

        DeploymentManifest {
            framework: Framework::AwsSam,
            content,
            format: ManifestFormat::Yaml,
            files: vec![],
        }
    }

    fn generate_dockerfile(&self, function: &ServerlessFunction) -> String {
        format!(
            "FROM public.ecr.aws/lambda/provided:al2\nCOPY {name}.wasm /var/task/function.wasm\nCOPY bootstrap /var/runtime/bootstrap\nCMD [\"bootstrap\"]\n",
            name = function.name,
        )
    }

    fn validate(&self, function: &ServerlessFunction) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if function.name.is_empty() {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "Function name is required".to_string(),
                field: Some("name".to_string()),
            });
        }
        if function.runtime.memory_mb < 128 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "AWS Lambda minimum memory is 128MB".to_string(),
                field: Some("runtime.memory_mb".to_string()),
            });
        }
        if function.runtime.memory_mb > 10240 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                message: "Memory exceeds AWS Lambda maximum (10240MB)".to_string(),
                field: Some("runtime.memory_mb".to_string()),
            });
        }
        if function.runtime.timeout.as_secs() > 900 {
            issues.push(ValidationIssue {
                severity: IssueSeverity::Error,
                message: "AWS Lambda maximum timeout is 900 seconds".to_string(),
                field: Some("runtime.timeout".to_string()),
            });
        }
        issues
    }
}

/// Factory function to get an adapter for a given framework.
pub fn get_adapter(framework: Framework) -> Box<dyn FrameworkAdapter> {
    match framework {
        Framework::OpenFaaS => Box::new(OpenFaaSAdapter),
        Framework::Knative => Box::new(KnativeAdapter),
        Framework::Fission => Box::new(FissionAdapter),
        Framework::AwsSam => Box::new(AwsSamAdapter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverless::function::{FunctionBuilder, HttpMethod};

    fn sample_function() -> ServerlessFunction {
        FunctionBuilder::new("test-func")
            .description("A test function")
            .module_path("./test-func.wasm")
            .memory_mb(256)
            .http_trigger("/api/test", vec![HttpMethod::Get, HttpMethod::Post])
            .env("ENV_VAR", "value")
            .build()
    }

    #[test]
    fn test_openfaas_manifest_contains_function_name() {
        let adapter = OpenFaaSAdapter;
        let func = sample_function();
        let manifest = adapter.generate_manifest(&func);

        assert_eq!(manifest.framework, Framework::OpenFaaS);
        assert_eq!(manifest.format, ManifestFormat::Yaml);
        assert!(manifest.content.contains("test-func"));
        assert!(manifest.content.contains("256Mi"));
    }

    #[test]
    fn test_knative_manifest_contains_scaling() {
        let adapter = KnativeAdapter;
        let func = sample_function();
        let manifest = adapter.generate_manifest(&func);

        assert_eq!(manifest.framework, Framework::Knative);
        assert!(manifest.content.contains("serving.knative.dev/v1"));
        assert!(manifest.content.contains("test-func"));
        assert!(manifest.content.contains("256Mi"));
    }

    #[test]
    fn test_fission_manifest_contains_triggers() {
        let adapter = FissionAdapter;
        let func = sample_function();
        let manifest = adapter.generate_manifest(&func);

        assert_eq!(manifest.framework, Framework::Fission);
        assert!(manifest.content.contains("fission.io/v1"));
        assert!(manifest.content.contains("/api/test"));
    }

    #[test]
    fn test_aws_sam_manifest_contains_resources() {
        let adapter = AwsSamAdapter;
        let func = sample_function();
        let manifest = adapter.generate_manifest(&func);

        assert_eq!(manifest.framework, Framework::AwsSam);
        assert!(manifest.content.contains("AWS::Serverless::Function"));
        assert!(manifest.content.contains("256"));
    }

    #[test]
    fn test_openfaas_dockerfile() {
        let adapter = OpenFaaSAdapter;
        let func = sample_function();
        let dockerfile = adapter.generate_dockerfile(&func);

        assert!(dockerfile.contains("openfaas"));
        assert!(dockerfile.contains("test-func.wasm"));
    }

    #[test]
    fn test_knative_dockerfile() {
        let adapter = KnativeAdapter;
        let func = sample_function();
        let dockerfile = adapter.generate_dockerfile(&func);

        assert!(dockerfile.contains("isolate-runtime"));
        assert!(dockerfile.contains("8080"));
    }

    #[test]
    fn test_aws_sam_validation_low_memory() {
        let adapter = AwsSamAdapter;
        let func = FunctionBuilder::new("low-mem").memory_mb(64).build();

        let issues = adapter.validate(&func);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("128MB")));
    }

    #[test]
    fn test_aws_sam_validation_high_timeout() {
        let adapter = AwsSamAdapter;
        let func =
            FunctionBuilder::new("slow-func").timeout(std::time::Duration::from_secs(1000)).build();

        let issues = adapter.validate(&func);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("900")));
    }

    #[test]
    fn test_validation_empty_name() {
        let func = FunctionBuilder::new("").build();

        let adapters: Vec<Box<dyn FrameworkAdapter>> = vec![
            Box::new(OpenFaaSAdapter),
            Box::new(KnativeAdapter),
            Box::new(FissionAdapter),
            Box::new(AwsSamAdapter),
        ];

        for adapter in &adapters {
            let issues = adapter.validate(&func);
            assert!(
                issues
                    .iter()
                    .any(|i| i.severity == IssueSeverity::Error && i.message.contains("name")),
                "Adapter {} should report empty name error",
                adapter.framework()
            );
        }
    }

    #[test]
    fn test_get_adapter_factory() {
        let adapter = get_adapter(Framework::OpenFaaS);
        assert_eq!(adapter.framework(), Framework::OpenFaaS);

        let adapter = get_adapter(Framework::Knative);
        assert_eq!(adapter.framework(), Framework::Knative);

        let adapter = get_adapter(Framework::Fission);
        assert_eq!(adapter.framework(), Framework::Fission);

        let adapter = get_adapter(Framework::AwsSam);
        assert_eq!(adapter.framework(), Framework::AwsSam);
    }

    #[test]
    fn test_fission_long_name_validation() {
        let long_name = "a".repeat(64);
        let func = FunctionBuilder::new(long_name).build();
        let adapter = FissionAdapter;
        let issues = adapter.validate(&func);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("63")));
    }

    #[test]
    fn test_framework_display() {
        assert_eq!(Framework::OpenFaaS.to_string(), "OpenFaaS");
        assert_eq!(Framework::Knative.to_string(), "Knative");
        assert_eq!(Framework::Fission.to_string(), "Fission");
        assert_eq!(Framework::AwsSam.to_string(), "AWS SAM");
    }
}
