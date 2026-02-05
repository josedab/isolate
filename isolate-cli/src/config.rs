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
    // Search for config file in current directory and parents
    let mut current_dir = std::env::current_dir().ok()?;

    loop {
        let config_path = current_dir.join(".isolate.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).ok()?;
            return toml::from_str(&content).ok();
        }

        if !current_dir.pop() {
            break;
        }
    }

    None
}
