//! Visual capability designer backend.
//!
//! Provides policy templates, a builder for constructing capability policies,
//! a template gallery with pre-built defaults, multi-format export, and
//! policy validation with warnings.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::capability::types::Capability;

// ---------------------------------------------------------------------------
// Policy template
// ---------------------------------------------------------------------------

/// A reusable policy template describing a set of capabilities and resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTemplate {
    /// Human-readable name for this template.
    pub name: String,
    /// Description of what this template is intended for.
    pub description: String,
    /// High-level category.
    pub category: TemplateCategory,
    /// Capabilities granted by this template.
    pub capabilities: Vec<Capability>,
    /// Optional resource limits.
    pub resource_limits: Option<PolicyResourceLimits>,
    /// Free-form tags for search and filtering.
    pub tags: Vec<String>,
}

/// Category of a policy template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateCategory {
    /// Bare minimum — no I/O, pure computation.
    Minimal,
    /// Web-facing service — HTTP, stdout/stderr.
    WebService,
    /// Data processing — filesystem read, stdout.
    DataProcessing,
    /// AI agent — network, filesystem, stdout.
    AiAgent,
    /// Full access — everything (use with caution).
    FullAccess,
    /// User-defined.
    Custom,
}

/// Resource limits that can be attached to a policy template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyResourceLimits {
    /// Maximum memory in megabytes.
    pub memory_mb: Option<u64>,
    /// Maximum CPU time in seconds.
    pub cpu_time_secs: Option<u64>,
    /// Maximum fuel (instruction count budget).
    pub fuel: Option<u64>,
    /// Maximum I/O read in megabytes.
    pub io_read_mb: Option<u64>,
    /// Maximum I/O write in megabytes.
    pub io_write_mb: Option<u64>,
}

// ---------------------------------------------------------------------------
// Policy builder
// ---------------------------------------------------------------------------

/// Builder for constructing a [`PolicyTemplate`] programmatically.
#[derive(Debug, Clone)]
pub struct PolicyBuilder {
    capabilities: Vec<Capability>,
    resource_limits: Option<PolicyResourceLimits>,
    name: String,
    description: String,
}

impl PolicyBuilder {
    /// Create a new builder with the given template name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            capabilities: Vec::new(),
            resource_limits: None,
            name: name.into(),
            description: String::new(),
        }
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add a capability.
    pub fn add_capability(mut self, cap: Capability) -> Self {
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
        self
    }

    /// Remove a capability (if present).
    pub fn remove_capability(mut self, cap: &Capability) -> Self {
        self.capabilities.retain(|c| c != cap);
        self
    }

    /// Set resource limits.
    pub fn set_resource_limits(mut self, limits: PolicyResourceLimits) -> Self {
        self.resource_limits = Some(limits);
        self
    }

    /// Allow stdout.
    pub fn allow_stdout(self) -> Self {
        self.add_capability(Capability::stdout())
    }

    /// Allow stderr.
    pub fn allow_stderr(self) -> Self {
        self.add_capability(Capability::stderr())
    }

    /// Allow stdin.
    pub fn allow_stdin(self) -> Self {
        self.add_capability(Capability::stdin())
    }

    /// Allow read-only filesystem access at `path`.
    pub fn allow_filesystem_read(self, path: impl Into<PathBuf>) -> Self {
        self.add_capability(Capability::filesystem_read(path.into()))
    }

    /// Allow read-write filesystem access at `path`.
    pub fn allow_filesystem_write(self, path: impl Into<PathBuf>) -> Self {
        self.add_capability(Capability::filesystem_write(path.into()))
    }

    /// Allow network (HTTP) access to the given hosts.
    pub fn allow_network(self, hosts: Vec<impl Into<String>>) -> Self {
        self.add_capability(Capability::http_client(hosts))
    }

    /// Allow time access (system clock, monotonic clock, timers).
    pub fn allow_time(self) -> Self {
        self.add_capability(Capability::system_clock())
            .add_capability(Capability::monotonic_clock())
            .add_capability(Capability::timers())
    }

    /// Allow random number generation (secure).
    pub fn allow_random(self) -> Self {
        self.add_capability(Capability::secure_random())
    }

    /// Build the [`PolicyTemplate`].
    pub fn build(self) -> PolicyTemplate {
        let category = infer_category(&self.capabilities);
        PolicyTemplate {
            name: self.name,
            description: self.description,
            category,
            capabilities: self.capabilities,
            resource_limits: self.resource_limits,
            tags: Vec::new(),
        }
    }
}

/// Heuristically infer a category from the granted capabilities.
fn infer_category(caps: &[Capability]) -> TemplateCategory {
    if caps.is_empty() {
        return TemplateCategory::Minimal;
    }

    let has_network = caps.iter().any(|c| matches!(c, Capability::Network(_)));
    let has_fs_write = caps.iter().any(|c| {
        matches!(
            c,
            Capability::Filesystem(crate::capability::types::FilesystemCapability::ReadWrite(_))
        )
    });
    let has_fs_read = caps.iter().any(|c| matches!(c, Capability::Filesystem(_)));
    let has_time = caps.iter().any(|c| matches!(c, Capability::Time(_)));
    let has_random = caps.iter().any(|c| matches!(c, Capability::Random(_)));

    // Full access: network + fs write + time + random
    if has_network && has_fs_write && has_time && has_random {
        return TemplateCategory::FullAccess;
    }
    // AI agent: network + fs (any) + at least time or random
    if has_network && has_fs_read && (has_time || has_random) {
        return TemplateCategory::AiAgent;
    }
    // Web service: network, no fs
    if has_network && !has_fs_read {
        return TemplateCategory::WebService;
    }
    // Data processing: fs read, no network
    if has_fs_read && !has_network {
        return TemplateCategory::DataProcessing;
    }

    TemplateCategory::Custom
}

// ---------------------------------------------------------------------------
// Template gallery
// ---------------------------------------------------------------------------

/// A gallery of pre-built and user-added policy templates.
#[derive(Debug, Clone)]
pub struct TemplateGallery {
    templates: Vec<PolicyTemplate>,
}

impl Default for TemplateGallery {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateGallery {
    /// Create a gallery pre-populated with the default templates.
    pub fn new() -> Self {
        Self { templates: default_templates() }
    }

    /// Add a custom template.
    pub fn add_template(&mut self, template: PolicyTemplate) {
        self.templates.push(template);
    }

    /// Get a template by name.
    pub fn get_template(&self, name: &str) -> Option<&PolicyTemplate> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// Search templates whose name, description, or tags contain `query` (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<&PolicyTemplate> {
        let q = query.to_lowercase();
        self.templates
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&q)
                    || t.description.to_lowercase().contains(&q)
                    || t.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Get all templates in a given category.
    pub fn by_category(&self, category: &TemplateCategory) -> Vec<&PolicyTemplate> {
        self.templates.iter().filter(|t| &t.category == category).collect()
    }

    /// List all templates.
    pub fn list_all(&self) -> &[PolicyTemplate] {
        &self.templates
    }
}

/// Build the six default templates shipped with Isolate.
fn default_templates() -> Vec<PolicyTemplate> {
    vec![
        // 1. Minimal
        PolicyTemplate {
            name: "minimal".into(),
            description: "No capabilities — pure computation only.".into(),
            category: TemplateCategory::Minimal,
            capabilities: vec![],
            resource_limits: Some(PolicyResourceLimits {
                memory_mb: Some(64),
                cpu_time_secs: Some(30),
                fuel: Some(1_000_000),
                io_read_mb: None,
                io_write_mb: None,
            }),
            tags: vec!["safe".into(), "compute".into()],
        },
        // 2. Hello World
        PolicyTemplate {
            name: "hello-world".into(),
            description: "Stdout only — for simple programs that print output.".into(),
            category: TemplateCategory::Minimal,
            capabilities: vec![Capability::stdout()],
            resource_limits: Some(PolicyResourceLimits {
                memory_mb: Some(64),
                cpu_time_secs: Some(30),
                fuel: Some(1_000_000),
                io_read_mb: None,
                io_write_mb: Some(1),
            }),
            tags: vec!["safe".into(), "beginner".into()],
        },
        // 3. Web Service
        PolicyTemplate {
            name: "web-service".into(),
            description: "HTTP client access with stdout and stderr.".into(),
            category: TemplateCategory::WebService,
            capabilities: vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::http_client(vec!["*"]),
                Capability::dns_resolve(),
            ],
            resource_limits: Some(PolicyResourceLimits {
                memory_mb: Some(128),
                cpu_time_secs: Some(60),
                fuel: Some(10_000_000),
                io_read_mb: Some(10),
                io_write_mb: Some(10),
            }),
            tags: vec!["network".into(), "http".into()],
        },
        // 4. Data Pipeline
        PolicyTemplate {
            name: "data-pipeline".into(),
            description: "Filesystem read with stdout and stderr for data processing.".into(),
            category: TemplateCategory::DataProcessing,
            capabilities: vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::filesystem_read("/data"),
            ],
            resource_limits: Some(PolicyResourceLimits {
                memory_mb: Some(256),
                cpu_time_secs: Some(300),
                fuel: Some(100_000_000),
                io_read_mb: Some(100),
                io_write_mb: Some(10),
            }),
            tags: vec!["data".into(), "etl".into(), "batch".into()],
        },
        // 5. AI Agent
        PolicyTemplate {
            name: "ai-agent".into(),
            description: "Full agent capabilities — network, filesystem, time, random.".into(),
            category: TemplateCategory::AiAgent,
            capabilities: vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::http_client(vec!["*"]),
                Capability::dns_resolve(),
                Capability::filesystem_read("/data"),
                Capability::filesystem_write("/workspace"),
                Capability::system_clock(),
                Capability::monotonic_clock(),
                Capability::timers(),
                Capability::secure_random(),
            ],
            resource_limits: Some(PolicyResourceLimits {
                memory_mb: Some(512),
                cpu_time_secs: Some(600),
                fuel: Some(1_000_000_000),
                io_read_mb: Some(500),
                io_write_mb: Some(100),
            }),
            tags: vec!["ai".into(), "agent".into(), "llm".into()],
        },
        // 6. Full Access
        PolicyTemplate {
            name: "full-access".into(),
            description: "All capabilities enabled — use with caution.".into(),
            category: TemplateCategory::FullAccess,
            capabilities: vec![
                Capability::stdin(),
                Capability::stdout(),
                Capability::stderr(),
                Capability::http_client(vec!["*"]),
                Capability::dns_resolve(),
                Capability::filesystem_read("/"),
                Capability::filesystem_write("/"),
                Capability::temp_dir(),
                Capability::system_clock(),
                Capability::monotonic_clock(),
                Capability::timers(),
                Capability::secure_random(),
                Capability::env_all(),
                Capability::args(),
            ],
            resource_limits: None,
            tags: vec!["unrestricted".into(), "dangerous".into()],
        },
    ]
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// JSON (always available).
    Json,
    /// YAML-like output (rendered via serde_json structure; for true YAML enable the
    /// `platform` or `kubernetes` feature which pulls in `serde_yaml`).
    Yaml,
    /// TOML-style output (manually formatted — no `toml` crate dependency).
    Toml,
    /// Compilable Rust source code.
    RustCode,
}

/// Exports a [`PolicyTemplate`] to various formats.
pub struct PolicyExporter;

impl PolicyExporter {
    /// Export a template in the requested format.
    pub fn export(template: &PolicyTemplate, format: ExportFormat) -> crate::Result<String> {
        match format {
            ExportFormat::Json => Self::export_json(template),
            ExportFormat::Yaml => Self::export_yaml(template),
            ExportFormat::Toml => Self::export_toml(template),
            ExportFormat::RustCode => Self::export_rust_code(template),
        }
    }

    /// Export as pretty-printed JSON.
    pub fn export_json(template: &PolicyTemplate) -> crate::Result<String> {
        serde_json::to_string_pretty(template)
            .map_err(|e| crate::Error::InvalidConfig(format!("JSON serialization failed: {e}")))
    }

    /// Export as YAML-like output.
    ///
    /// When the `serde_yaml` crate is available (behind a feature flag) this
    /// would use it directly.  Without it we produce a simplified YAML-style
    /// representation derived from the JSON serialization.
    pub fn export_yaml(template: &PolicyTemplate) -> crate::Result<String> {
        // Produce a simple YAML-like format from JSON values.
        let value: serde_json::Value = serde_json::to_value(template)
            .map_err(|e| crate::Error::InvalidConfig(format!("serialization failed: {e}")))?;
        let mut out = String::new();
        write_yaml_value(&mut out, &value, 0);
        Ok(out)
    }

    /// Export as TOML-style output (manually formatted).
    pub fn export_toml(template: &PolicyTemplate) -> crate::Result<String> {
        let mut out = String::new();
        out.push_str("[policy]\n");
        out.push_str(&format!("name = \"{}\"\n", template.name));
        out.push_str(&format!("description = \"{}\"\n", template.description));
        out.push_str(&format!("category = \"{:?}\"\n", template.category));

        if !template.tags.is_empty() {
            let tags: Vec<String> = template.tags.iter().map(|t| format!("\"{}\"", t)).collect();
            out.push_str(&format!("tags = [{}]\n", tags.join(", ")));
        }

        out.push('\n');

        for (i, cap) in template.capabilities.iter().enumerate() {
            out.push_str("[[policy.capabilities]]\n");
            out.push_str(&format!("# {}\n", cap.description()));
            out.push_str(&format!("type = \"{}\"\n", capability_type_label(cap)));
            out.push_str(&format!("index = {}\n", i));
            out.push('\n');
        }

        if let Some(ref limits) = template.resource_limits {
            out.push_str("[policy.resource_limits]\n");
            if let Some(v) = limits.memory_mb {
                out.push_str(&format!("memory_mb = {}\n", v));
            }
            if let Some(v) = limits.cpu_time_secs {
                out.push_str(&format!("cpu_time_secs = {}\n", v));
            }
            if let Some(v) = limits.fuel {
                out.push_str(&format!("fuel = {}\n", v));
            }
            if let Some(v) = limits.io_read_mb {
                out.push_str(&format!("io_read_mb = {}\n", v));
            }
            if let Some(v) = limits.io_write_mb {
                out.push_str(&format!("io_write_mb = {}\n", v));
            }
        }

        Ok(out)
    }

    /// Export as compilable Rust source code.
    pub fn export_rust_code(template: &PolicyTemplate) -> crate::Result<String> {
        let mut out = String::new();
        out.push_str("use isolate_core::capability::Capability;\n");
        out.push_str("use isolate_core::capability::designer::{PolicyTemplate, PolicyResourceLimits, TemplateCategory};\n\n");
        out.push_str(&format!("/// Auto-generated policy template: {}\n", template.name));
        out.push_str(&format!(
            "pub fn {}_policy() -> PolicyTemplate {{\n",
            template.name.replace('-', "_")
        ));
        out.push_str("    PolicyTemplate {\n");
        out.push_str(&format!("        name: \"{}\".into(),\n", template.name));
        out.push_str(&format!("        description: \"{}\".into(),\n", template.description));
        out.push_str(&format!("        category: TemplateCategory::{:?},\n", template.category));

        // capabilities
        out.push_str("        capabilities: vec![\n");
        for cap in &template.capabilities {
            out.push_str(&format!("            {},\n", capability_to_rust(cap)));
        }
        out.push_str("        ],\n");

        // resource limits
        match &template.resource_limits {
            Some(limits) => {
                out.push_str("        resource_limits: Some(PolicyResourceLimits {\n");
                out.push_str(&format!("            memory_mb: {:?},\n", limits.memory_mb));
                out.push_str(&format!("            cpu_time_secs: {:?},\n", limits.cpu_time_secs));
                out.push_str(&format!("            fuel: {:?},\n", limits.fuel));
                out.push_str(&format!("            io_read_mb: {:?},\n", limits.io_read_mb));
                out.push_str(&format!("            io_write_mb: {:?},\n", limits.io_write_mb));
                out.push_str("        }),\n");
            }
            None => {
                out.push_str("        resource_limits: None,\n");
            }
        }

        // tags
        let tags: Vec<String> = template.tags.iter().map(|t| format!("\"{}\".into()", t)).collect();
        out.push_str(&format!("        tags: vec![{}],\n", tags.join(", ")));

        out.push_str("    }\n");
        out.push_str("}\n");

        Ok(out)
    }
}

/// Recursively write a serde_json::Value as simplified YAML.
fn write_yaml_value(out: &mut String, value: &serde_json::Value, indent: usize) {
    let pad = "  ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                match val {
                    serde_json::Value::Object(_) => {
                        out.push_str(&format!("{}{}:\n", pad, key));
                        write_yaml_value(out, val, indent + 1);
                    }
                    serde_json::Value::Array(arr) => {
                        out.push_str(&format!("{}{}:\n", pad, key));
                        for item in arr {
                            match item {
                                serde_json::Value::Object(_) => {
                                    out.push_str(&format!("{}- \n", pad));
                                    write_yaml_value(out, item, indent + 2);
                                }
                                _ => {
                                    out.push_str(&format!("{}- {}\n", pad, yaml_scalar(item)));
                                }
                            }
                        }
                    }
                    _ => {
                        out.push_str(&format!("{}{}: {}\n", pad, key, yaml_scalar(val)));
                    }
                }
            }
        }
        _ => {
            out.push_str(&format!("{}{}\n", pad, yaml_scalar(value)));
        }
    }
}

/// Format a scalar JSON value as a YAML scalar string.
fn yaml_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("\"{}\"", s),
        other => format!("{}", other),
    }
}

/// Return a short human-readable label for a capability variant.
fn capability_type_label(cap: &Capability) -> &'static str {
    match cap {
        Capability::Filesystem(_) => "filesystem",
        Capability::Network(_) => "network",
        Capability::Time(_) => "time",
        Capability::Random(_) => "random",
        Capability::Environment(_) => "environment",
        Capability::Stdio(_) => "stdio",
        Capability::HostFunction(_) => "host_function",
    }
}

/// Convert a capability to a Rust expression string.
fn capability_to_rust(cap: &Capability) -> String {
    match cap {
        Capability::Stdio(s) => match s {
            crate::capability::types::StdioCapability::Stdin => "Capability::stdin()".into(),
            crate::capability::types::StdioCapability::Stdout => "Capability::stdout()".into(),
            crate::capability::types::StdioCapability::Stderr => "Capability::stderr()".into(),
        },
        Capability::Filesystem(fs) => match fs {
            crate::capability::types::FilesystemCapability::ReadOnly(p) => {
                format!("Capability::filesystem_read(\"{}\")", p.display())
            }
            crate::capability::types::FilesystemCapability::ReadWrite(p) => {
                format!("Capability::filesystem_write(\"{}\")", p.display())
            }
            crate::capability::types::FilesystemCapability::TempDir => {
                "Capability::temp_dir()".into()
            }
        },
        Capability::Network(net) => match net {
            crate::capability::types::NetworkCapability::HttpClient(hosts) => {
                let host_strs: Vec<String> = hosts.iter().map(|h| format!("\"{}\"", h)).collect();
                format!("Capability::http_client(vec![{}])", host_strs.join(", "))
            }
            crate::capability::types::NetworkCapability::DnsResolve => {
                "Capability::dns_resolve()".into()
            }
            crate::capability::types::NetworkCapability::TcpListen(port) => {
                format!("Capability::tcp_listen({})", port)
            }
            crate::capability::types::NetworkCapability::TcpConnect(addrs) => {
                let addr_strs: Vec<String> =
                    addrs.iter().map(|a| format!("\"{}\".parse().unwrap()", a)).collect();
                format!("Capability::tcp_connect(vec![{}])", addr_strs.join(", "))
            }
        },
        Capability::Time(t) => match t {
            crate::capability::types::TimeCapability::SystemClock => {
                "Capability::system_clock()".into()
            }
            crate::capability::types::TimeCapability::MonotonicClock => {
                "Capability::monotonic_clock()".into()
            }
            crate::capability::types::TimeCapability::Timers => "Capability::timers()".into(),
        },
        Capability::Random(r) => match r {
            crate::capability::types::RandomCapability::Secure => {
                "Capability::secure_random()".into()
            }
            crate::capability::types::RandomCapability::Seeded(seed) => {
                format!("Capability::seeded_random({})", seed)
            }
        },
        Capability::Environment(env) => match env {
            crate::capability::types::EnvironmentCapability::ReadVar(name) => {
                format!("Capability::env_var(\"{}\")", name)
            }
            crate::capability::types::EnvironmentCapability::ReadAll => {
                "Capability::env_all()".into()
            }
            crate::capability::types::EnvironmentCapability::Args => "Capability::args()".into(),
        },
        Capability::HostFunction(hf) => match hf {
            crate::capability::types::HostFunctionCapability::Named(name) => {
                format!("Capability::host_function(\"{}\")", name)
            }
            crate::capability::types::HostFunctionCapability::Namespace(ns) => {
                // No convenience constructor — use struct literal.
                format!(
                    "Capability::HostFunction(isolate_core::capability::types::HostFunctionCapability::Namespace(\"{}\".into()))",
                    ns
                )
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validates a [`PolicyTemplate`] and returns actionable warnings.
pub struct PolicyValidator;

impl PolicyValidator {
    /// Validate a template and return any warnings.
    pub fn validate(template: &PolicyTemplate) -> Vec<PolicyWarning> {
        let mut warnings = Vec::new();

        Self::check_empty_capabilities(template, &mut warnings);
        Self::check_missing_stdout_for_debug(template, &mut warnings);
        Self::check_overly_permissive(template, &mut warnings);
        Self::check_wildcard_network(template, &mut warnings);
        Self::check_root_filesystem(template, &mut warnings);
        Self::check_conflicting_capabilities(template, &mut warnings);
        Self::check_missing_resource_limits(template, &mut warnings);
        Self::check_env_all(template, &mut warnings);

        warnings
    }

    fn check_empty_capabilities(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        if template.capabilities.is_empty() && template.category != TemplateCategory::Minimal {
            warnings.push(PolicyWarning {
                level: WarningLevel::Info,
                message: "Template has no capabilities — sandbox will have no I/O access.".into(),
                suggestion: Some("Add at least Capability::stdout() for basic output.".into()),
            });
        }
    }

    fn check_missing_stdout_for_debug(
        template: &PolicyTemplate,
        warnings: &mut Vec<PolicyWarning>,
    ) {
        let has_stdout = template.capabilities.iter().any(|c| {
            matches!(c, Capability::Stdio(crate::capability::types::StdioCapability::Stdout))
        });
        let has_stderr = template.capabilities.iter().any(|c| {
            matches!(c, Capability::Stdio(crate::capability::types::StdioCapability::Stderr))
        });

        if !template.capabilities.is_empty() && !has_stdout && !has_stderr {
            warnings.push(PolicyWarning {
                level: WarningLevel::Warning,
                message: "No stdout or stderr — debugging will be difficult.".into(),
                suggestion: Some(
                    "Consider adding Capability::stdout() or Capability::stderr().".into(),
                ),
            });
        }
    }

    fn check_overly_permissive(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        let has_network = template.capabilities.iter().any(|c| matches!(c, Capability::Network(_)));
        let has_fs_write = template.capabilities.iter().any(|c| {
            matches!(
                c,
                Capability::Filesystem(crate::capability::types::FilesystemCapability::ReadWrite(
                    _
                ))
            )
        });
        let has_env_all = template.capabilities.iter().any(|c| {
            matches!(
                c,
                Capability::Environment(crate::capability::types::EnvironmentCapability::ReadAll)
            )
        });

        if has_network && has_fs_write && has_env_all {
            warnings.push(PolicyWarning {
                level: WarningLevel::Critical,
                message: "Highly permissive: network + filesystem write + environment read. \
                          Data exfiltration risk."
                    .into(),
                suggestion: Some(
                    "Restrict network hosts, filesystem paths, or remove env_all.".into(),
                ),
            });
        }
    }

    fn check_wildcard_network(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        for cap in &template.capabilities {
            if let Capability::Network(crate::capability::types::NetworkCapability::HttpClient(
                hosts,
            )) = cap
            {
                if hosts.iter().any(|h| h == "*") {
                    warnings.push(PolicyWarning {
                        level: WarningLevel::Warning,
                        message: "Wildcard HTTP host '*' allows connections to any host.".into(),
                        suggestion: Some(
                            "Restrict to specific hosts, e.g. vec![\"api.example.com\"].".into(),
                        ),
                    });
                }
            }
        }
    }

    fn check_root_filesystem(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        for cap in &template.capabilities {
            match cap {
                Capability::Filesystem(
                    crate::capability::types::FilesystemCapability::ReadWrite(p),
                ) if p.as_os_str() == "/" => {
                    warnings.push(PolicyWarning {
                        level: WarningLevel::Critical,
                        message: "Read-write access to '/' grants full filesystem write.".into(),
                        suggestion: Some(
                            "Restrict to a specific directory, e.g. \"/workspace\".".into(),
                        ),
                    });
                }
                Capability::Filesystem(
                    crate::capability::types::FilesystemCapability::ReadOnly(p),
                ) if p.as_os_str() == "/" => {
                    warnings.push(PolicyWarning {
                        level: WarningLevel::Warning,
                        message: "Read access to '/' exposes the entire filesystem.".into(),
                        suggestion: Some(
                            "Restrict to specific directories, e.g. \"/data\".".into(),
                        ),
                    });
                }
                _ => {}
            }
        }
    }

    fn check_conflicting_capabilities(
        template: &PolicyTemplate,
        warnings: &mut Vec<PolicyWarning>,
    ) {
        // Check for both read-only and read-write to the same path.
        let read_paths: Vec<&PathBuf> = template
            .capabilities
            .iter()
            .filter_map(|c| match c {
                Capability::Filesystem(
                    crate::capability::types::FilesystemCapability::ReadOnly(p),
                ) => Some(p),
                _ => None,
            })
            .collect();

        let write_paths: Vec<&PathBuf> = template
            .capabilities
            .iter()
            .filter_map(|c| match c {
                Capability::Filesystem(
                    crate::capability::types::FilesystemCapability::ReadWrite(p),
                ) => Some(p),
                _ => None,
            })
            .collect();

        for rp in &read_paths {
            for wp in &write_paths {
                if rp == wp {
                    warnings.push(PolicyWarning {
                        level: WarningLevel::Info,
                        message: format!(
                            "Redundant: ReadOnly(\"{}\") is superseded by ReadWrite for the same path.",
                            rp.display()
                        ),
                        suggestion: Some("Remove the ReadOnly capability for this path.".into()),
                    });
                }
            }
        }
    }

    fn check_missing_resource_limits(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        if template.resource_limits.is_none() && template.category != TemplateCategory::FullAccess {
            warnings.push(PolicyWarning {
                level: WarningLevel::Warning,
                message: "No resource limits set — sandbox may consume unlimited resources.".into(),
                suggestion: Some(
                    "Set memory, CPU, and fuel limits via PolicyResourceLimits.".into(),
                ),
            });
        }
    }

    fn check_env_all(template: &PolicyTemplate, warnings: &mut Vec<PolicyWarning>) {
        let has_env_all = template.capabilities.iter().any(|c| {
            matches!(
                c,
                Capability::Environment(crate::capability::types::EnvironmentCapability::ReadAll)
            )
        });
        if has_env_all {
            warnings.push(PolicyWarning {
                level: WarningLevel::Warning,
                message: "env_all exposes all environment variables, which may include secrets."
                    .into(),
                suggestion: Some(
                    "Use Capability::env_var(\"NAME\") to grant access to specific variables."
                        .into(),
                ),
            });
        }
    }
}

/// A warning produced by [`PolicyValidator`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyWarning {
    /// Severity level.
    pub level: WarningLevel,
    /// Human-readable message.
    pub message: String,
    /// Optional suggestion for resolving the warning.
    pub suggestion: Option<String>,
}

/// Severity level for a [`PolicyWarning`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningLevel {
    /// Informational — no action required.
    Info,
    /// Warning — should be reviewed.
    Warning,
    /// Critical — likely a security issue.
    Critical,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gallery_has_default_templates() {
        let gallery = TemplateGallery::new();
        assert_eq!(gallery.list_all().len(), 6);
    }

    #[test]
    fn test_gallery_get_template_by_name() {
        let gallery = TemplateGallery::new();
        let minimal = gallery.get_template("minimal");
        assert!(minimal.is_some());
        assert_eq!(minimal.unwrap().name, "minimal");
        assert!(gallery.get_template("nonexistent").is_none());
    }

    #[test]
    fn test_gallery_search() {
        let gallery = TemplateGallery::new();
        let results = gallery.search("agent");
        assert!(!results.is_empty());
        assert!(results.iter().any(|t| t.name == "ai-agent"));
    }

    #[test]
    fn test_gallery_by_category() {
        let gallery = TemplateGallery::new();
        let web = gallery.by_category(&TemplateCategory::WebService);
        assert_eq!(web.len(), 1);
        assert_eq!(web[0].name, "web-service");
    }

    #[test]
    fn test_gallery_add_custom_template() {
        let mut gallery = TemplateGallery::new();
        let custom =
            PolicyBuilder::new("custom").description("A custom template").allow_stdout().build();
        gallery.add_template(custom);
        assert_eq!(gallery.list_all().len(), 7);
        assert!(gallery.get_template("custom").is_some());
    }

    #[test]
    fn test_policy_builder() {
        let policy = PolicyBuilder::new("my-policy")
            .description("Test policy")
            .allow_stdout()
            .allow_stderr()
            .allow_filesystem_read("/data")
            .allow_time()
            .allow_random()
            .set_resource_limits(PolicyResourceLimits {
                memory_mb: Some(128),
                cpu_time_secs: Some(60),
                fuel: Some(5_000_000),
                io_read_mb: None,
                io_write_mb: None,
            })
            .build();

        assert_eq!(policy.name, "my-policy");
        assert_eq!(policy.description, "Test policy");
        assert!(!policy.capabilities.is_empty());
        assert!(policy.resource_limits.is_some());
    }

    #[test]
    fn test_policy_builder_remove_capability() {
        let policy = PolicyBuilder::new("test")
            .allow_stdout()
            .allow_stderr()
            .remove_capability(&Capability::stderr())
            .build();

        assert!(policy.capabilities.contains(&Capability::stdout()));
        assert!(!policy.capabilities.contains(&Capability::stderr()));
    }

    #[test]
    fn test_policy_builder_no_duplicates() {
        let policy = PolicyBuilder::new("test").allow_stdout().allow_stdout().build();

        let stdout_count =
            policy.capabilities.iter().filter(|c| c == &&Capability::stdout()).count();
        assert_eq!(stdout_count, 1);
    }

    #[test]
    fn test_export_json() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("hello-world").unwrap();
        let json = PolicyExporter::export(template, ExportFormat::Json).unwrap();
        assert!(json.contains("hello-world"));
        assert!(json.contains("Stdout"));
    }

    #[test]
    fn test_export_yaml() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("minimal").unwrap();
        let yaml = PolicyExporter::export(template, ExportFormat::Yaml).unwrap();
        assert!(yaml.contains("name:"));
        assert!(yaml.contains("\"minimal\""));
    }

    #[test]
    fn test_export_toml() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("data-pipeline").unwrap();
        let toml = PolicyExporter::export(template, ExportFormat::Toml).unwrap();
        assert!(toml.contains("[policy]"));
        assert!(toml.contains("name = \"data-pipeline\""));
    }

    #[test]
    fn test_export_rust_code() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("hello-world").unwrap();
        let code = PolicyExporter::export(template, ExportFormat::RustCode).unwrap();
        assert!(code.contains("fn hello_world_policy()"));
        assert!(code.contains("Capability::stdout()"));
    }

    #[test]
    fn test_validate_minimal_no_warnings() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("minimal").unwrap();
        let warnings = PolicyValidator::validate(template);
        // Minimal template should have no warnings.
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn test_validate_full_access_warnings() {
        let gallery = TemplateGallery::new();
        let template = gallery.get_template("full-access").unwrap();
        let warnings = PolicyValidator::validate(template);
        // Full access should trigger several warnings.
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.level == WarningLevel::Critical));
    }

    #[test]
    fn test_validate_conflicting_capabilities() {
        let template = PolicyBuilder::new("conflict")
            .add_capability(Capability::filesystem_read("/data"))
            .add_capability(Capability::filesystem_write("/data"))
            .allow_stdout()
            .build();
        let warnings = PolicyValidator::validate(&template);
        assert!(warnings.iter().any(|w| w.message.contains("Redundant")));
    }

    #[test]
    fn test_validate_missing_output() {
        let template = PolicyTemplate {
            name: "no-output".into(),
            description: "No stdout or stderr".into(),
            category: TemplateCategory::Custom,
            capabilities: vec![Capability::filesystem_read("/data")],
            resource_limits: None,
            tags: vec![],
        };
        let warnings = PolicyValidator::validate(&template);
        assert!(warnings.iter().any(|w| w.message.contains("stdout")));
    }

    #[test]
    fn test_infer_category() {
        assert_eq!(infer_category(&[]), TemplateCategory::Minimal);
        assert_eq!(
            infer_category(&[Capability::http_client(vec!["*"])]),
            TemplateCategory::WebService
        );
        assert_eq!(
            infer_category(&[Capability::filesystem_read("/data")]),
            TemplateCategory::DataProcessing
        );
    }
}
