//! Declarative sandbox-as-code configuration.
//!
//! Load sandbox configurations from YAML files with support for:
//! - Inheritance (extend from base configurations)
//! - Template variables with `${VAR}` syntax
//! - Environment-specific overrides
//! - Multiple sandbox definitions in a single file

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A complete sandbox-as-code definition file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxFile {
    /// File format version.
    #[serde(default = "default_version")]
    pub version: String,

    /// Template variables (can be overridden by environment).
    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// Base configurations that can be inherited.
    #[serde(default)]
    pub templates: HashMap<String, SandboxSpec>,

    /// Named sandbox definitions.
    pub sandboxes: HashMap<String, SandboxSpec>,

    /// Environment-specific overrides.
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentOverride>,
}

fn default_version() -> String {
    "1".to_string()
}

/// A single sandbox specification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxSpec {
    /// Inherit from a named template.
    #[serde(rename = "extends")]
    pub extends: Option<String>,

    /// Path or URI to the WASM module.
    #[serde(default)]
    pub module: Option<String>,

    /// Entry point function name.
    #[serde(default)]
    pub entry_point: Option<String>,

    /// Resource limits.
    #[serde(default)]
    pub resources: Option<ResourceSpec>,

    /// Capabilities to grant.
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// Environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Metadata/labels.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Resource limits specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    /// Memory limit (e.g., "128MB", "1GB").
    #[serde(default)]
    pub memory: Option<String>,

    /// CPU fuel units.
    #[serde(default)]
    pub fuel: Option<u64>,

    /// Wall time limit (e.g., "30s", "5m").
    #[serde(default)]
    pub timeout: Option<String>,

    /// I/O write limit (e.g., "1MB").
    #[serde(default)]
    pub io_write_limit: Option<String>,

    /// I/O read limit (e.g., "10MB").
    #[serde(default)]
    pub io_read_limit: Option<String>,
}

/// Environment-specific overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentOverride {
    /// Variables to override.
    #[serde(default)]
    pub variables: HashMap<String, String>,

    /// Per-sandbox overrides.
    #[serde(default)]
    pub sandboxes: HashMap<String, SandboxSpec>,
}

/// Errors from loading/parsing sandbox-as-code files.
#[derive(Debug, Clone)]
pub enum ConfigError {
    /// YAML parsing error.
    Parse(String),
    /// Template not found.
    TemplateNotFound(String),
    /// Circular inheritance detected.
    CircularInheritance(String),
    /// Variable not defined.
    UndefinedVariable(String),
    /// Invalid resource value.
    InvalidResource(String),
    /// IO error.
    Io(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Parse(e) => write!(f, "parse error: {}", e),
            ConfigError::TemplateNotFound(t) => write!(f, "template not found: {}", t),
            ConfigError::CircularInheritance(c) => write!(f, "circular inheritance: {}", c),
            ConfigError::UndefinedVariable(v) => write!(f, "undefined variable: ${{{}}}", v),
            ConfigError::InvalidResource(r) => write!(f, "invalid resource value: {}", r),
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Loader for sandbox-as-code configuration files.
pub struct ConfigLoader {
    /// Additional variables injected at load time (e.g., from CLI or env).
    extra_vars: HashMap<String, String>,
    /// Active environment name.
    environment: Option<String>,
}

impl ConfigLoader {
    /// Create a new config loader.
    pub fn new() -> Self {
        Self {
            extra_vars: HashMap::new(),
            environment: None,
        }
    }

    /// Set the active environment for overrides.
    pub fn with_environment(mut self, env: impl Into<String>) -> Self {
        self.environment = Some(env.into());
        self
    }

    /// Add a variable override.
    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_vars.insert(key.into(), value.into());
        self
    }

    /// Load from a YAML string.
    pub fn load_yaml(&self, yaml: &str) -> Result<SandboxFile, ConfigError> {
        let mut file: SandboxFile =
            serde_yaml::from_str(yaml).map_err(|e| ConfigError::Parse(e.to_string()))?;

        // Merge extra variables (overrides file variables)
        for (k, v) in &self.extra_vars {
            file.variables.insert(k.clone(), v.clone());
        }

        // Apply environment overrides
        if let Some(env_name) = &self.environment {
            if let Some(env_override) = file.environments.get(env_name).cloned() {
                for (k, v) in env_override.variables {
                    file.variables.insert(k, v);
                }
                for (name, spec_override) in env_override.sandboxes {
                    if let Some(base) = file.sandboxes.get_mut(&name) {
                        merge_spec(base, &spec_override);
                    }
                }
            }
        }

        // Resolve inheritance
        let template_names: Vec<String> = file.sandboxes.keys().cloned().collect();
        for name in &template_names {
            self.resolve_inheritance(&mut file, name)?;
        }

        // Substitute variables in all sandbox specs
        let vars = file.variables.clone();
        for spec in file.sandboxes.values_mut() {
            self.substitute_variables(spec, &vars)?;
        }

        Ok(file)
    }

    /// Load from a YAML file path.
    pub fn load_file(&self, path: &Path) -> Result<SandboxFile, ConfigError> {
        let content =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        self.load_yaml(&content)
    }

    /// Resolve inheritance for a sandbox spec.
    fn resolve_inheritance(
        &self,
        file: &mut SandboxFile,
        name: &str,
    ) -> Result<(), ConfigError> {
        let spec = file
            .sandboxes
            .get(name)
            .ok_or_else(|| ConfigError::TemplateNotFound(name.to_string()))?
            .clone();

        if let Some(ref parent_name) = spec.extends {
            // Check for circular inheritance
            let mut chain = vec![name.to_string()];
            let mut current = parent_name.clone();
            loop {
                if chain.contains(&current) {
                    return Err(ConfigError::CircularInheritance(chain.join(" -> ")));
                }
                let parent = file
                    .templates
                    .get(&current)
                    .or_else(|| file.sandboxes.get(&current))
                    .ok_or_else(|| ConfigError::TemplateNotFound(current.clone()))?
                    .clone();
                chain.push(current.clone());
                if let Some(ref next) = parent.extends {
                    current = next.clone();
                } else {
                    break;
                }
            }

            // Build resolved spec by merging parent chain
            let parent = file
                .templates
                .get(parent_name)
                .or_else(|| file.sandboxes.get(parent_name))
                .ok_or_else(|| ConfigError::TemplateNotFound(parent_name.clone()))?
                .clone();

            let mut resolved = parent;
            merge_spec(&mut resolved, &spec);
            resolved.extends = None; // Clear inheritance marker
            file.sandboxes.insert(name.to_string(), resolved);
        }

        Ok(())
    }

    /// Substitute `${VAR}` placeholders in a spec.
    fn substitute_variables(
        &self,
        spec: &mut SandboxSpec,
        vars: &HashMap<String, String>,
    ) -> Result<(), ConfigError> {
        if let Some(ref mut module) = spec.module {
            *module = substitute(module, vars)?;
        }
        if let Some(ref mut ep) = spec.entry_point {
            *ep = substitute(ep, vars)?;
        }
        let env_clone = spec.env.clone();
        spec.env.clear();
        for (k, v) in env_clone {
            spec.env
                .insert(substitute(&k, vars)?, substitute(&v, vars)?);
        }
        let args_clone = spec.args.clone();
        spec.args.clear();
        for a in args_clone {
            spec.args.push(substitute(&a, vars)?);
        }
        Ok(())
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge an override spec into a base spec. Override values take precedence.
fn merge_spec(base: &mut SandboxSpec, over: &SandboxSpec) {
    if over.module.is_some() {
        base.module = over.module.clone();
    }
    if over.entry_point.is_some() {
        base.entry_point = over.entry_point.clone();
    }
    if over.resources.is_some() {
        base.resources = over.resources.clone();
    }
    if !over.capabilities.is_empty() {
        // Merge capabilities (deduplicate)
        for cap in &over.capabilities {
            if !base.capabilities.contains(cap) {
                base.capabilities.push(cap.clone());
            }
        }
    }
    for (k, v) in &over.env {
        base.env.insert(k.clone(), v.clone());
    }
    if !over.args.is_empty() {
        base.args = over.args.clone();
    }
    for (k, v) in &over.labels {
        base.labels.insert(k.clone(), v.clone());
    }
}

/// Substitute `${VAR}` patterns in a string.
fn substitute(input: &str, vars: &HashMap<String, String>) -> Result<String, ConfigError> {
    let mut result = input.to_string();
    // Find all ${...} patterns
    loop {
        let start = match result.find("${") {
            Some(i) => i,
            None => break,
        };
        let end = match result[start..].find('}') {
            Some(i) => start + i,
            None => break,
        };
        let var_name = &result[start + 2..end];
        let value = vars
            .get(var_name)
            .ok_or_else(|| ConfigError::UndefinedVariable(var_name.to_string()))?;
        result = format!("{}{}{}", &result[..start], value, &result[end + 1..]);
    }
    Ok(result)
}

/// Parse a human-readable size string (e.g., "128MB", "1GB") to bytes.
pub fn parse_size(s: &str) -> Result<u64, ConfigError> {
    let s = s.trim();
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("GB") {
        (n.trim(), 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n.trim(), 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n.trim(), 1024)
    } else if let Some(n) = s.strip_suffix('B') {
        (n.trim(), 1)
    } else {
        // Assume bytes
        (s, 1)
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| ConfigError::InvalidResource(s.to_string()))?;
    Ok(num * multiplier)
}

/// Parse a human-readable duration string (e.g., "30s", "5m") to Duration.
pub fn parse_duration(s: &str) -> Result<std::time::Duration, ConfigError> {
    let s = s.trim();
    // Check ms before m and s to avoid partial matches
    if let Some(n) = s.strip_suffix("ms") {
        let num: u64 = n
            .trim()
            .parse()
            .map_err(|_| ConfigError::InvalidResource(s.to_string()))?;
        return Ok(std::time::Duration::from_millis(num));
    }
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('h') {
        (n.trim(), 3600)
    } else if let Some(n) = s.strip_suffix('m') {
        (n.trim(), 60)
    } else if let Some(n) = s.strip_suffix('s') {
        (n.trim(), 1)
    } else {
        // Assume seconds
        (s, 1)
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| ConfigError::InvalidResource(s.to_string()))?;
    Ok(std::time::Duration::from_secs(num * multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC_YAML: &str = r#"
version: "1"
variables:
  MODULE_PATH: "./modules"
  TIMEOUT: "30s"
sandboxes:
  hello:
    module: "${MODULE_PATH}/hello.wasm"
    resources:
      memory: "128MB"
      fuel: 10000000
      timeout: "${TIMEOUT}"
    capabilities:
      - stdout
      - stderr
    env:
      API_KEY: test-key
"#;

    const INHERITANCE_YAML: &str = r#"
version: "1"
templates:
  base:
    resources:
      memory: "64MB"
      fuel: 1000000
    capabilities:
      - stdout
    env:
      ENV: production
sandboxes:
  worker:
    extends: base
    module: worker.wasm
    capabilities:
      - fs-read:/data
    env:
      WORKER_ID: "1"
"#;

    const ENV_OVERRIDE_YAML: &str = r#"
version: "1"
variables:
  LOG_LEVEL: info
sandboxes:
  app:
    module: app.wasm
    env:
      LOG_LEVEL: "${LOG_LEVEL}"
environments:
  development:
    variables:
      LOG_LEVEL: debug
  production:
    variables:
      LOG_LEVEL: warn
    sandboxes:
      app:
        resources:
          memory: "256MB"
"#;

    #[test]
    fn test_basic_yaml_loading() {
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(BASIC_YAML).unwrap();

        assert_eq!(file.version, "1");
        assert!(file.sandboxes.contains_key("hello"));

        let hello = &file.sandboxes["hello"];
        assert_eq!(hello.module.as_deref(), Some("./modules/hello.wasm"));
        assert!(hello.capabilities.contains(&"stdout".to_string()));
        assert_eq!(hello.env.get("API_KEY").unwrap(), "test-key");
    }

    #[test]
    fn test_variable_substitution() {
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(BASIC_YAML).unwrap();
        let hello = &file.sandboxes["hello"];
        assert_eq!(hello.module.as_deref(), Some("./modules/hello.wasm"));
    }

    #[test]
    fn test_variable_override() {
        let loader = ConfigLoader::new()
            .with_variable("MODULE_PATH", "/custom/path");
        let file = loader.load_yaml(BASIC_YAML).unwrap();
        let hello = &file.sandboxes["hello"];
        assert_eq!(hello.module.as_deref(), Some("/custom/path/hello.wasm"));
    }

    #[test]
    fn test_inheritance() {
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(INHERITANCE_YAML).unwrap();

        let worker = &file.sandboxes["worker"];
        // Module from child
        assert_eq!(worker.module.as_deref(), Some("worker.wasm"));
        // Capabilities merged from parent + child
        assert!(worker.capabilities.contains(&"stdout".to_string()));
        assert!(worker.capabilities.contains(&"fs-read:/data".to_string()));
        // Env merged
        assert_eq!(worker.env.get("ENV").unwrap(), "production");
        assert_eq!(worker.env.get("WORKER_ID").unwrap(), "1");
        // Resources from parent
        assert!(worker.resources.is_some());
    }

    #[test]
    fn test_environment_override() {
        let loader = ConfigLoader::new().with_environment("production");
        let file = loader.load_yaml(ENV_OVERRIDE_YAML).unwrap();

        let app = &file.sandboxes["app"];
        assert_eq!(app.env.get("LOG_LEVEL").unwrap(), "warn");
        // Production adds memory override
        assert!(app.resources.is_some());
        assert_eq!(
            app.resources.as_ref().unwrap().memory.as_deref(),
            Some("256MB")
        );
    }

    #[test]
    fn test_development_environment() {
        let loader = ConfigLoader::new().with_environment("development");
        let file = loader.load_yaml(ENV_OVERRIDE_YAML).unwrap();

        let app = &file.sandboxes["app"];
        assert_eq!(app.env.get("LOG_LEVEL").unwrap(), "debug");
    }

    #[test]
    fn test_undefined_variable_error() {
        let yaml = r#"
sandboxes:
  test:
    module: "${NONEXISTENT}/module.wasm"
"#;
        let loader = ConfigLoader::new();
        let result = loader.load_yaml(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_template_not_found_error() {
        let yaml = r#"
sandboxes:
  test:
    extends: nonexistent_template
    module: test.wasm
"#;
        let loader = ConfigLoader::new();
        let result = loader.load_yaml(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("128MB").unwrap(), 128 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
        assert_eq!(parse_size("1024B").unwrap(), 1024);
        assert_eq!(parse_size("4096").unwrap(), 4096);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), std::time::Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), std::time::Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), std::time::Duration::from_secs(3600));
        assert_eq!(parse_duration("100ms").unwrap(), std::time::Duration::from_millis(100));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("notanumber").is_err());
    }

    #[test]
    fn test_empty_sandboxes() {
        let yaml = r#"
sandboxes:
  minimal:
    module: test.wasm
"#;
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(yaml).unwrap();
        assert!(file.sandboxes.contains_key("minimal"));
        let spec = &file.sandboxes["minimal"];
        assert!(spec.capabilities.is_empty());
        assert!(spec.env.is_empty());
    }

    #[test]
    fn test_multiple_sandboxes() {
        let yaml = r#"
sandboxes:
  worker-a:
    module: a.wasm
  worker-b:
    module: b.wasm
  worker-c:
    module: c.wasm
"#;
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(yaml).unwrap();
        assert_eq!(file.sandboxes.len(), 3);
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::Parse("bad yaml".into());
        assert!(err.to_string().contains("parse error"));

        let err = ConfigError::UndefinedVariable("FOO".into());
        assert!(err.to_string().contains("${FOO}"));
    }

    #[test]
    fn test_parse_size_zero() {
        assert_eq!(parse_size("0B").unwrap(), 0);
        assert_eq!(parse_size("0").unwrap(), 0);
        assert_eq!(parse_size("0MB").unwrap(), 0);
    }

    #[test]
    fn test_parse_size_with_whitespace() {
        assert_eq!(parse_size("  128MB  ").unwrap(), 128 * 1024 * 1024);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("notanumber").is_err());
    }

    #[test]
    fn test_parse_duration_zero() {
        assert_eq!(parse_duration("0s").unwrap(), std::time::Duration::from_secs(0));
        assert_eq!(parse_duration("0ms").unwrap(), std::time::Duration::from_millis(0));
    }

    #[test]
    fn test_parse_duration_plain_number() {
        // Plain number treated as seconds
        assert_eq!(parse_duration("60").unwrap(), std::time::Duration::from_secs(60));
    }

    #[test]
    fn test_nested_variable_substitution() {
        let yaml = r#"
variables:
  BASE_PATH: /opt
  MODULE_DIR: "${BASE_PATH}/modules"
sandboxes:
  app:
    module: test.wasm
    env:
      PATH: "${BASE_PATH}/bin"
"#;
        let loader = ConfigLoader::new();
        let file = loader.load_yaml(yaml).unwrap();
        let app = &file.sandboxes["app"];
        assert_eq!(app.env.get("PATH").unwrap(), "/opt/bin");
    }

    #[test]
    fn test_config_error_all_variants_display() {
        let errors = vec![
            ConfigError::TemplateNotFound("base".into()),
            ConfigError::CircularInheritance("a -> b -> a".into()),
            ConfigError::InvalidResource("xyz".into()),
            ConfigError::Io("permission denied".into()),
        ];
        for err in &errors {
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn test_merge_spec_selective() {
        let mut base = SandboxSpec {
            module: Some("base.wasm".into()),
            entry_point: Some("_start".into()),
            capabilities: vec!["stdout".into()],
            env: [("A".to_string(), "1".to_string())].into_iter().collect(),
            ..Default::default()
        };
        let over = SandboxSpec {
            module: Some("override.wasm".into()),
            capabilities: vec!["stderr".into()],
            env: [("B".to_string(), "2".to_string())].into_iter().collect(),
            ..Default::default()
        };
        merge_spec(&mut base, &over);

        assert_eq!(base.module.as_deref(), Some("override.wasm"));
        assert_eq!(base.entry_point.as_deref(), Some("_start")); // Not overridden
        assert!(base.capabilities.contains(&"stdout".to_string()));
        assert!(base.capabilities.contains(&"stderr".to_string()));
        assert_eq!(base.env.get("A"), Some(&"1".to_string()));
        assert_eq!(base.env.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn test_invalid_yaml_syntax() {
        let loader = ConfigLoader::new();
        let result = loader.load_yaml("not: valid: yaml: [[[");
        assert!(result.is_err());
    }
}
