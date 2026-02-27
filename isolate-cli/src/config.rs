use serde::Deserialize;

/// Configuration file structures for .isolate.toml
#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
pub struct ProjectConfig {
    pub project: Option<ProjectInfo>,
    pub sandbox: Option<SandboxDefaults>,
    pub modules: Option<Vec<ModuleConfig>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
pub struct ProjectInfo {
    pub name: Option<String>,
    pub version: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
pub struct SandboxDefaults {
    pub memory_limit: Option<String>,
    pub timeout: Option<u64>,
    pub fuel: Option<u64>,
    pub cpu_time: Option<u64>,
    pub entry_point: Option<String>,
    pub capabilities: Option<CapabilitiesConfig>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub args: Option<ArgsConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CapabilitiesConfig {
    pub stdout: Option<bool>,
    pub stderr: Option<bool>,
    pub stdin: Option<bool>,
    pub time: Option<bool>,
    pub random: Option<bool>,
    pub dns: Option<bool>,
    pub fs: Option<FsCapabilities>,
    pub http: Option<HttpCapabilities>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FsCapabilities {
    pub read: Option<Vec<String>>,
    pub write: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct HttpCapabilities {
    pub hosts: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ArgsConfig {
    pub values: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ModuleConfig {
    pub name: String,
    pub path: String,
    pub memory_limit: Option<String>,
    pub timeout: Option<u64>,
    pub fuel: Option<u64>,
}

/// Load project configuration from .isolate.toml
pub fn load_project_config() -> Option<ProjectConfig> {
    load_project_config_from(std::env::current_dir().ok()?)
}

/// Load project configuration starting from a given directory, searching parents.
pub fn load_project_config_from(start_dir: std::path::PathBuf) -> Option<ProjectConfig> {
    let mut current_dir = start_dir;

    loop {
        let config_path = current_dir.join(".isolate.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).ok()?;
            return parse_project_config(&content).ok();
        }

        if !current_dir.pop() {
            break;
        }
    }

    None
}

/// Parse a TOML string into a [`ProjectConfig`].
pub fn parse_project_config(content: &str) -> Result<ProjectConfig, toml::de::Error> {
    toml::from_str(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config_parsing() {
        let toml = r#"
[project]
name = "my-sandbox"
version = "1.0.0"

[sandbox]
memory_limit = "256M"
timeout = 30
fuel = 1000000
entry_point = "_start"
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        let project = config.project.unwrap();
        assert_eq!(project.name.as_deref(), Some("my-sandbox"));
        assert_eq!(project.version.as_deref(), Some("1.0.0"));
        let sandbox = config.sandbox.unwrap();
        assert_eq!(sandbox.memory_limit.as_deref(), Some("256M"));
        assert_eq!(sandbox.timeout, Some(30));
        assert_eq!(sandbox.fuel, Some(1_000_000));
    }

    #[test]
    fn test_invalid_toml_returns_error() {
        let bad_toml = "this is [not valid {{ toml";
        let result = parse_project_config(bad_toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_config_is_valid() {
        let config: ProjectConfig = toml::from_str("").unwrap();
        assert!(config.project.is_none());
        assert!(config.sandbox.is_none());
        assert!(config.modules.is_none());
    }

    #[test]
    fn test_missing_config_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_project_config_from(dir.path().to_path_buf());
        assert!(result.is_none());
    }

    #[test]
    fn test_parent_directory_search() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("sub").join("deep");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(dir.path().join(".isolate.toml"), "[project]\nname = \"found\"\n").unwrap();

        let result = load_project_config_from(child);
        let config = result.expect("should find config in parent");
        assert_eq!(config.project.unwrap().name.as_deref(), Some("found"));
    }

    #[test]
    fn test_capabilities_config_parsing() {
        let toml = r#"
[sandbox.capabilities]
stdout = true
stderr = false
stdin = true
time = true
random = false

[sandbox.capabilities.fs]
read = ["/data", "/tmp"]
write = ["/output"]

[sandbox.capabilities.http]
hosts = ["api.example.com"]
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        let caps = config.sandbox.unwrap().capabilities.unwrap();
        assert_eq!(caps.stdout, Some(true));
        assert_eq!(caps.stderr, Some(false));
        let fs = caps.fs.unwrap();
        assert_eq!(fs.read.unwrap().len(), 2);
        assert_eq!(fs.write.unwrap(), vec!["/output"]);
        assert_eq!(caps.http.unwrap().hosts.unwrap(), vec!["api.example.com"]);
    }

    #[test]
    fn test_modules_config_parsing() {
        let toml = r#"
[[modules]]
name = "worker"
path = "./worker.wasm"
memory_limit = "128M"
timeout = 10
fuel = 500000
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        let modules = config.modules.unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "worker");
        assert_eq!(modules[0].path, "./worker.wasm");
        assert_eq!(modules[0].memory_limit.as_deref(), Some("128M"));
    }

    #[test]
    fn test_invalid_timeout_type_returns_error() {
        let toml = r#"
[sandbox]
timeout = "not-a-number"
"#;
        let result = parse_project_config(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_with_env_vars() {
        let toml = r#"
[sandbox.env]
MY_VAR = "value"
OTHER = "123"
"#;
        let config: ProjectConfig = toml::from_str(toml).unwrap();
        let env = config.sandbox.unwrap().env.unwrap();
        assert_eq!(env.get("MY_VAR").unwrap(), "value");
        assert_eq!(env.get("OTHER").unwrap(), "123");
    }
}
