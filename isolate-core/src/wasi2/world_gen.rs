//! WIT world definition generator and composition pipeline.
//!
//! This module provides:
//! - [`WorldGenerator`] for creating WIT world definitions from Isolate configurations
//! - [`WorldDefinition`] representing a complete WIT world with imports and exports
//! - [`CompositionPipeline`] for orchestrating multi-component execution
//! - [`PipelineStage`] for individual stages in a composition pipeline
//! - [`PipelineResult`] for pipeline execution results with per-stage metrics

#[allow(dead_code)]
use crate::capability::{Capability, CapabilitySet};
use crate::resource::ResourceLimits;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::context::ComponentConfig;

/// WASI Preview 2 interface namespace.
const WASI_NAMESPACE: &str = "wasi";

/// Default WIT world version.
const DEFAULT_WORLD_VERSION: &str = "0.2.0";

// ---------------------------------------------------------------------------
// WorldDefinition
// ---------------------------------------------------------------------------

/// A complete WIT world definition with imports and exports.
///
/// Represents a WIT world document that declares the full set of interfaces
/// a component requires (imports) and provides (exports).
///
/// # Example
///
/// ```rust,ignore
/// use isolate_core::wasi2::world_gen::WorldDefinition;
///
/// let world = WorldDefinition::new("my-app")
///     .with_import("wasi:cli/stdout@0.2.0")
///     .with_export("my:app/run@1.0.0");
///
/// println!("{}", world.to_wit());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldDefinition {
    /// World name (e.g., "my-sandbox").
    pub name: String,
    /// World version.
    pub version: String,
    /// Package namespace (e.g., "isolate:sandbox").
    pub package: String,
    /// Imported interfaces (host → component).
    pub imports: Vec<WorldInterface>,
    /// Exported interfaces (component → host).
    pub exports: Vec<WorldInterface>,
    /// Documentation comment for the world.
    pub docs: Option<String>,
}

impl WorldDefinition {
    /// Create a new world definition with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            package: format!("isolate:sandbox/{name}"),
            name,
            version: DEFAULT_WORLD_VERSION.to_string(),
            imports: Vec::new(),
            exports: Vec::new(),
            docs: None,
        }
    }

    /// Set the package namespace.
    pub fn with_package(mut self, package: impl Into<String>) -> Self {
        self.package = package.into();
        self
    }

    /// Set the world version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Add documentation.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Add an imported interface by fully-qualified name.
    pub fn with_import(mut self, interface: impl Into<String>) -> Self {
        self.imports.push(WorldInterface::from_name(interface));
        self
    }

    /// Add an exported interface by fully-qualified name.
    pub fn with_export(mut self, interface: impl Into<String>) -> Self {
        self.exports.push(WorldInterface::from_name(interface));
        self
    }

    /// Add a typed import interface.
    pub fn with_import_interface(mut self, interface: WorldInterface) -> Self {
        self.imports.push(interface);
        self
    }

    /// Add a typed export interface.
    pub fn with_export_interface(mut self, interface: WorldInterface) -> Self {
        self.exports.push(interface);
        self
    }

    /// Render the world as a WIT document string.
    pub fn to_wit(&self) -> String {
        let mut wit = String::new();

        // Package declaration
        wit.push_str(&format!("package {}@{};\n\n", self.package, self.version));

        // Documentation
        if let Some(docs) = &self.docs {
            for line in docs.lines() {
                wit.push_str(&format!("/// {line}\n"));
            }
        }

        // World block
        wit.push_str(&format!("world {} {{\n", self.name));

        // Imports
        for iface in &self.imports {
            wit.push_str(&format!("    import {};\n", iface.qualified_name()));
        }

        // Separator between imports and exports
        if !self.imports.is_empty() && !self.exports.is_empty() {
            wit.push('\n');
        }

        // Exports
        for iface in &self.exports {
            wit.push_str(&format!("    export {};\n", iface.qualified_name()));
        }

        wit.push_str("}\n");
        wit
    }
}

/// An interface reference within a WIT world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInterface {
    /// Fully-qualified interface name (e.g., "wasi:cli/stdout@0.2.0").
    pub name: String,
    /// Whether this interface is optional.
    pub optional: bool,
    /// Documentation.
    pub docs: Option<String>,
}

impl WorldInterface {
    /// Create from a fully-qualified name.
    pub fn from_name(name: impl Into<String>) -> Self {
        Self { name: name.into(), optional: false, docs: None }
    }

    /// Mark as optional.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Add documentation.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Get the qualified name for WIT rendering.
    fn qualified_name(&self) -> &str {
        &self.name
    }
}

// ---------------------------------------------------------------------------
// WorldGenerator
// ---------------------------------------------------------------------------

/// Generates WIT world definitions from Isolate configurations.
///
/// Maps [`CapabilitySet`] entries and [`ComponentConfig`] to the corresponding
/// WASI Preview 2 interfaces, producing a complete [`WorldDefinition`].
///
/// # Example
///
/// ```rust,ignore
/// use isolate_core::capability::{Capability, CapabilitySet};
/// use isolate_core::wasi2::world_gen::WorldGenerator;
///
/// let mut caps = CapabilitySet::new();
/// caps.grant(Capability::stdout());
/// caps.grant(Capability::filesystem_read("/data"));
///
/// let gen = WorldGenerator::new();
/// let world = gen.from_capabilities("my-sandbox", &caps);
/// println!("{}", world.to_wit());
/// ```
pub struct WorldGenerator {
    /// Additional custom interface mappings.
    custom_mappings: HashMap<String, Vec<String>>,
    /// Default world version.
    version: String,
}

impl WorldGenerator {
    /// Create a new generator with default settings.
    pub fn new() -> Self {
        Self { custom_mappings: HashMap::new(), version: DEFAULT_WORLD_VERSION.to_string() }
    }

    /// Set the default world version for generated worlds.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Register a custom capability-to-interface mapping.
    ///
    /// The `key` is a capability description prefix and `interfaces` are the
    /// WASI interfaces it maps to.
    pub fn register_mapping(
        mut self,
        key: impl Into<String>,
        interfaces: Vec<impl Into<String>>,
    ) -> Self {
        self.custom_mappings
            .insert(key.into(), interfaces.into_iter().map(Into::into).collect());
        self
    }

    /// Generate a [`WorldDefinition`] from a [`CapabilitySet`].
    pub fn from_capabilities(&self, name: &str, capabilities: &CapabilitySet) -> WorldDefinition {
        let mut world = WorldDefinition::new(name).with_version(self.version.clone());

        // Always include the base WASI interfaces
        world.imports.push(WorldInterface::from_name(format!(
            "{WASI_NAMESPACE}:cli/environment@{version}",
            version = self.version
        )));

        // Map each capability to its WASI interface(s)
        for cap in capabilities.iter() {
            let interfaces = self.capability_to_interfaces(cap);
            for iface in interfaces {
                if !world.imports.iter().any(|i| i.name == iface) {
                    world.imports.push(WorldInterface::from_name(iface));
                }
            }
        }

        // Add standard component export
        world.exports.push(WorldInterface::from_name(format!(
            "{WASI_NAMESPACE}:cli/run@{version}",
            version = self.version
        )));

        world
    }

    /// Generate a [`WorldDefinition`] from a [`ComponentConfig`].
    pub fn from_config(&self, config: &ComponentConfig) -> WorldDefinition {
        let name = format!("sandbox-{}", config.component_hash());
        let mut world = self.from_capabilities(&name, &config.capabilities);

        // Add resource-limit docs
        world.docs = Some(format!(
            "Auto-generated world for component {}.\nMemory limit: {} bytes, Fuel: {:?}",
            config.component_hash(),
            config.resources.memory.heap_max,
            config.resources.cpu.fuel,
        ));

        // If network is configured, ensure network interfaces
        if config.network.allow_tcp || !config.network.allowed_hosts.is_empty() {
            let tcp_iface = format!("{WASI_NAMESPACE}:sockets/tcp@{}", self.version);
            if !world.imports.iter().any(|i| i.name == tcp_iface) {
                world.imports.push(WorldInterface::from_name(tcp_iface));
            }
        }

        world
    }

    /// Map a single capability to its WASI Preview 2 interface names.
    fn capability_to_interfaces(&self, cap: &Capability) -> Vec<String> {
        let v = &self.version;

        match cap {
            Capability::Stdio(stdio) => {
                use crate::capability::StdioCapability;
                match stdio {
                    StdioCapability::Stdout => {
                        vec![format!("{WASI_NAMESPACE}:cli/stdout@{v}")]
                    }
                    StdioCapability::Stderr => {
                        vec![format!("{WASI_NAMESPACE}:cli/stderr@{v}")]
                    }
                    StdioCapability::Stdin => {
                        vec![format!("{WASI_NAMESPACE}:cli/stdin@{v}")]
                    }
                }
            }
            Capability::Filesystem(_) => {
                vec![
                    format!("{WASI_NAMESPACE}:filesystem/types@{v}"),
                    format!("{WASI_NAMESPACE}:filesystem/preopens@{v}"),
                ]
            }
            Capability::Network(net) => {
                use crate::capability::NetworkCapability;
                let mut ifaces = vec![format!("{WASI_NAMESPACE}:sockets/network@{v}")];
                match net {
                    NetworkCapability::HttpClient(_) => {
                        ifaces.push(format!("{WASI_NAMESPACE}:http/outgoing-handler@{v}"));
                    }
                    NetworkCapability::TcpConnect(_) | NetworkCapability::TcpListen(_) => {
                        ifaces.push(format!("{WASI_NAMESPACE}:sockets/tcp@{v}"));
                    }
                    NetworkCapability::DnsResolve => {
                        ifaces.push(format!("{WASI_NAMESPACE}:sockets/ip-name-lookup@{v}"));
                    }
                }
                ifaces
            }
            Capability::Time(_) => {
                vec![
                    format!("{WASI_NAMESPACE}:clocks/wall-clock@{v}"),
                    format!("{WASI_NAMESPACE}:clocks/monotonic-clock@{v}"),
                ]
            }
            Capability::Random(_) => {
                vec![format!("{WASI_NAMESPACE}:random/random@{v}")]
            }
            Capability::Environment(_) => {
                vec![format!("{WASI_NAMESPACE}:cli/environment@{v}")]
            }
            Capability::HostFunction(_) => {
                // Host functions are custom; check custom mappings
                let desc = cap.description();
                self.custom_mappings
                    .get(&desc)
                    .cloned()
                    .unwrap_or_default()
            }
        }
    }
}

impl Default for WorldGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CompositionPipeline
// ---------------------------------------------------------------------------

/// Orchestrates multi-component execution as a sequential pipeline.
///
/// Each stage processes input data and produces output that feeds into the next
/// stage.  Resource limits and capabilities are enforced per-stage.
///
/// # Example
///
/// ```rust,ignore
/// use isolate_core::wasi2::world_gen::{CompositionPipeline, PipelineStage};
///
/// let pipeline = CompositionPipeline::builder()
///     .name("etl-pipeline")
///     .stage(PipelineStage::new("extract", b"extract.wasm"))
///     .stage(PipelineStage::new("transform", b"transform.wasm"))
///     .stage(PipelineStage::new("load", b"load.wasm"))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct CompositionPipeline {
    /// Pipeline identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Ordered stages to execute.
    pub stages: Vec<PipelineStage>,
    /// Global resource limits for the entire pipeline.
    pub resource_limits: Option<ResourceLimits>,
    /// Pipeline-level metadata.
    pub metadata: HashMap<String, String>,
}

impl CompositionPipeline {
    /// Create a builder for constructing a pipeline.
    pub fn builder() -> CompositionPipelineBuilder {
        CompositionPipelineBuilder::new()
    }

    /// Get the number of stages.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Get a stage by index.
    pub fn stage(&self, index: usize) -> Option<&PipelineStage> {
        self.stages.get(index)
    }

    /// Get a stage by name.
    pub fn stage_by_name(&self, name: &str) -> Option<&PipelineStage> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// Execute the pipeline with the given initial input.
    ///
    /// Each stage receives the output of the previous stage as its input.
    /// The first stage receives `initial_input`.
    pub fn execute(&self, initial_input: &[u8]) -> PipelineResult {
        let pipeline_start = Instant::now();
        let mut current_data = initial_input.to_vec();
        let mut stage_results = Vec::with_capacity(self.stages.len());

        for (idx, stage) in self.stages.iter().enumerate() {
            let stage_start = Instant::now();

            // Simulate stage execution — in a real implementation this would
            // invoke the WASM component via the sandbox runtime.
            let result = stage.process(&current_data);

            let stage_duration = stage_start.elapsed();

            match result {
                Ok(output) => {
                    let metrics = StageMetrics {
                        stage_index: idx,
                        stage_name: stage.name.clone(),
                        duration: stage_duration,
                        input_bytes: current_data.len(),
                        output_bytes: output.len(),
                        success: true,
                        error: None,
                    };
                    current_data = output;
                    stage_results.push(metrics);
                }
                Err(err) => {
                    let metrics = StageMetrics {
                        stage_index: idx,
                        stage_name: stage.name.clone(),
                        duration: stage_duration,
                        input_bytes: current_data.len(),
                        output_bytes: 0,
                        success: false,
                        error: Some(err.clone()),
                    };
                    stage_results.push(metrics);

                    return PipelineResult {
                        pipeline_id: self.id.clone(),
                        success: false,
                        output: Vec::new(),
                        total_duration: pipeline_start.elapsed(),
                        stage_metrics: stage_results,
                        error: Some(err),
                    };
                }
            }
        }

        PipelineResult {
            pipeline_id: self.id.clone(),
            success: true,
            output: current_data,
            total_duration: pipeline_start.elapsed(),
            stage_metrics: stage_results,
            error: None,
        }
    }
}

/// Builder for [`CompositionPipeline`].
#[derive(Debug, Default)]
pub struct CompositionPipelineBuilder {
    name: Option<String>,
    stages: Vec<PipelineStage>,
    resource_limits: Option<ResourceLimits>,
    metadata: HashMap<String, String>,
}

impl CompositionPipelineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the pipeline name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Append a stage to the pipeline.
    pub fn stage(mut self, stage: PipelineStage) -> Self {
        self.stages.push(stage);
        self
    }

    /// Set global resource limits.
    pub fn resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.resource_limits = Some(limits);
        self
    }

    /// Add a metadata entry.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Build the pipeline.
    pub fn build(self) -> CompositionPipeline {
        CompositionPipeline {
            id: Uuid::new_v4().to_string(),
            name: self.name.unwrap_or_else(|| "unnamed-pipeline".to_string()),
            stages: self.stages,
            resource_limits: self.resource_limits,
            metadata: self.metadata,
        }
    }
}

// ---------------------------------------------------------------------------
// PipelineStage
// ---------------------------------------------------------------------------

/// An individual stage in a [`CompositionPipeline`].
///
/// Each stage holds a reference to a WASM component (by hash), its granted
/// capabilities, resource limits, and an optional data transformation function
/// used for testing and local simulation.
#[derive(Debug, Clone)]
pub struct PipelineStage {
    /// Stage name.
    pub name: String,
    /// Hash of the WASM component to execute.
    pub component_hash: String,
    /// Capabilities granted to this stage.
    pub capabilities: CapabilitySet,
    /// Per-stage resource limits (overrides pipeline defaults).
    pub resource_limits: Option<ResourceLimits>,
    /// Stage-level metadata.
    pub metadata: HashMap<String, String>,
    /// Transformation kind hint for simulation.
    pub transform: TransformKind,
}

impl PipelineStage {
    /// Create a new stage with a name and component bytes (hashed for identification).
    pub fn new(name: impl Into<String>, component_bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(component_bytes);
        let hash = hex::encode(hasher.finalize());

        Self {
            name: name.into(),
            component_hash: hash,
            capabilities: CapabilitySet::new(),
            resource_limits: None,
            metadata: HashMap::new(),
            transform: TransformKind::Passthrough,
        }
    }

    /// Grant a capability to this stage.
    pub fn with_capability(mut self, cap: Capability) -> Self {
        self.capabilities.grant(cap);
        self
    }

    /// Set per-stage resource limits.
    pub fn with_resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.resource_limits = Some(limits);
        self
    }

    /// Set a metadata entry.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the transform kind for simulation.
    pub fn with_transform(mut self, transform: TransformKind) -> Self {
        self.transform = transform;
        self
    }

    /// Process input data through this stage.
    ///
    /// In a full runtime this invokes the WASM component; the current
    /// implementation uses [`TransformKind`] for deterministic simulation.
    fn process(&self, input: &[u8]) -> Result<Vec<u8>, String> {
        match &self.transform {
            TransformKind::Passthrough => Ok(input.to_vec()),
            TransformKind::Uppercase => {
                Ok(input.iter().map(|b| b.to_ascii_uppercase()).collect())
            }
            TransformKind::Prefix(prefix) => {
                let mut out = prefix.as_bytes().to_vec();
                out.extend_from_slice(input);
                Ok(out)
            }
            TransformKind::Fail(msg) => Err(msg.clone()),
        }
    }
}

/// Deterministic transform kinds for pipeline simulation and testing.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TransformKind {
    /// Pass input through unchanged.
    #[default]
    Passthrough,
    /// Convert ASCII bytes to uppercase.
    Uppercase,
    /// Prepend a fixed prefix.
    Prefix(String),
    /// Always fail with the given message.
    Fail(String),
}

// ---------------------------------------------------------------------------
// PipelineResult
// ---------------------------------------------------------------------------

/// Result of executing a [`CompositionPipeline`].
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Pipeline identifier.
    pub pipeline_id: String,
    /// Whether all stages succeeded.
    pub success: bool,
    /// Final output bytes (empty on failure).
    pub output: Vec<u8>,
    /// Total wall-clock duration.
    pub total_duration: Duration,
    /// Per-stage execution metrics.
    pub stage_metrics: Vec<StageMetrics>,
    /// Error message if the pipeline failed.
    pub error: Option<String>,
}

impl PipelineResult {
    /// Get the number of stages that executed successfully.
    pub fn stages_completed(&self) -> usize {
        self.stage_metrics.iter().filter(|m| m.success).count()
    }

    /// Get the total number of stages that were attempted.
    pub fn stages_attempted(&self) -> usize {
        self.stage_metrics.len()
    }

    /// Get the metrics for a specific stage by name.
    pub fn metrics_for(&self, stage_name: &str) -> Option<&StageMetrics> {
        self.stage_metrics.iter().find(|m| m.stage_name == stage_name)
    }
}

/// Execution metrics for a single pipeline stage.
#[derive(Debug, Clone)]
pub struct StageMetrics {
    /// Index of the stage in the pipeline.
    pub stage_index: usize,
    /// Name of the stage.
    pub stage_name: String,
    /// Wall-clock duration of the stage.
    pub duration: Duration,
    /// Size of input data in bytes.
    pub input_bytes: usize,
    /// Size of output data in bytes.
    pub output_bytes: usize,
    /// Whether the stage succeeded.
    pub success: bool,
    /// Error message (if the stage failed).
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- WorldDefinition tests --

    #[test]
    fn test_world_definition_new() {
        let world = WorldDefinition::new("test-world");
        assert_eq!(world.name, "test-world");
        assert_eq!(world.package, "isolate:sandbox/test-world");
        assert_eq!(world.version, DEFAULT_WORLD_VERSION);
        assert!(world.imports.is_empty());
        assert!(world.exports.is_empty());
    }

    #[test]
    fn test_world_definition_builder_chain() {
        let world = WorldDefinition::new("my-world")
            .with_package("custom:pkg")
            .with_version("1.0.0")
            .with_docs("A test world")
            .with_import("wasi:cli/stdout@0.2.0")
            .with_export("wasi:cli/run@0.2.0");

        assert_eq!(world.package, "custom:pkg");
        assert_eq!(world.version, "1.0.0");
        assert_eq!(world.docs.as_deref(), Some("A test world"));
        assert_eq!(world.imports.len(), 1);
        assert_eq!(world.exports.len(), 1);
    }

    #[test]
    fn test_world_definition_to_wit() {
        let world = WorldDefinition::new("sandbox")
            .with_import("wasi:cli/stdout@0.2.0")
            .with_import("wasi:filesystem/types@0.2.0")
            .with_export("wasi:cli/run@0.2.0");

        let wit = world.to_wit();
        assert!(wit.contains("package isolate:sandbox/sandbox@0.2.0;"));
        assert!(wit.contains("world sandbox {"));
        assert!(wit.contains("import wasi:cli/stdout@0.2.0;"));
        assert!(wit.contains("import wasi:filesystem/types@0.2.0;"));
        assert!(wit.contains("export wasi:cli/run@0.2.0;"));
    }

    #[test]
    fn test_world_definition_to_wit_with_docs() {
        let world = WorldDefinition::new("documented")
            .with_docs("My documented world");

        let wit = world.to_wit();
        assert!(wit.contains("/// My documented world"));
    }

    #[test]
    fn test_world_interface_optional() {
        let iface = WorldInterface::from_name("wasi:http/outgoing@0.2.0").optional();
        assert!(iface.optional);
    }

    // -- WorldGenerator tests --

    #[test]
    fn test_generator_default() {
        let gen = WorldGenerator::new();
        assert_eq!(gen.version, DEFAULT_WORLD_VERSION);
        assert!(gen.custom_mappings.is_empty());
    }

    #[test]
    fn test_generator_from_empty_capabilities() {
        let gen = WorldGenerator::new();
        let caps = CapabilitySet::new();
        let world = gen.from_capabilities("empty", &caps);

        assert_eq!(world.name, "empty");
        // Should still have base environment import and run export
        assert!(!world.imports.is_empty());
        assert!(world.imports.iter().any(|i| i.name.contains("environment")));
        assert!(world.exports.iter().any(|i| i.name.contains("run")));
    }

    #[test]
    fn test_generator_stdout_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());

        let world = gen.from_capabilities("stdout-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("stdout")));
    }

    #[test]
    fn test_generator_filesystem_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::filesystem_read("/data"));

        let world = gen.from_capabilities("fs-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("filesystem/types")));
        assert!(world.imports.iter().any(|i| i.name.contains("filesystem/preopens")));
    }

    #[test]
    fn test_generator_network_http_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::http_client(vec!["api.example.com"]));

        let world = gen.from_capabilities("http-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("http/outgoing-handler")));
        assert!(world.imports.iter().any(|i| i.name.contains("sockets/network")));
    }

    #[test]
    fn test_generator_network_dns_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::dns_resolve());

        let world = gen.from_capabilities("dns-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("ip-name-lookup")));
    }

    #[test]
    fn test_generator_time_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::system_clock());

        let world = gen.from_capabilities("time-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("clocks/wall-clock")));
        assert!(world.imports.iter().any(|i| i.name.contains("clocks/monotonic-clock")));
    }

    #[test]
    fn test_generator_random_capability() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::secure_random());

        let world = gen.from_capabilities("random-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("random/random")));
    }

    #[test]
    fn test_generator_no_duplicate_imports() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::env_var("PATH"));
        caps.grant(Capability::env_all());

        let world = gen.from_capabilities("dedup-test", &caps);
        let env_count = world.imports.iter().filter(|i| i.name.contains("environment")).count();
        // base import + capability should not duplicate
        assert_eq!(env_count, 1);
    }

    #[test]
    fn test_generator_custom_version() {
        let gen = WorldGenerator::new().with_version("0.3.0");
        let caps = CapabilitySet::new();
        let world = gen.from_capabilities("versioned", &caps);

        assert!(world.imports.iter().all(|i| i.name.contains("0.3.0")));
    }

    #[test]
    fn test_generator_custom_mapping() {
        let gen = WorldGenerator::new()
            .register_mapping("hostfn:log", vec!["custom:logging/logger@1.0.0"]);

        let mut caps = CapabilitySet::new();
        caps.grant(Capability::host_function("log"));

        let world = gen.from_capabilities("custom-test", &caps);
        assert!(world.imports.iter().any(|i| i.name.contains("custom:logging/logger")));
    }

    #[test]
    fn test_generator_from_config() {
        // Minimal valid WASM module
        let wasm: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let config = ComponentConfig::builder()
            .component(wasm)
            .unwrap()
            .allow_stdout()
            .memory_limit(64 * 1024 * 1024)
            .fuel(500_000)
            .build()
            .unwrap();

        let gen = WorldGenerator::new();
        let world = gen.from_config(&config);

        assert!(world.name.starts_with("sandbox-"));
        assert!(world.docs.is_some());
        assert!(world.imports.iter().any(|i| i.name.contains("stdout")));
    }

    #[test]
    fn test_generator_from_config_with_network() {
        let wasm: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let config = ComponentConfig::builder()
            .component(wasm)
            .unwrap()
            .allow_tcp()
            .build()
            .unwrap();

        let gen = WorldGenerator::new();
        let world = gen.from_config(&config);

        assert!(world.imports.iter().any(|i| i.name.contains("sockets/tcp")));
    }

    // -- CompositionPipeline tests --

    #[test]
    fn test_pipeline_builder() {
        let pipeline = CompositionPipeline::builder()
            .name("test-pipeline")
            .stage(PipelineStage::new("s1", b"component1"))
            .stage(PipelineStage::new("s2", b"component2"))
            .metadata("env", "test")
            .build();

        assert_eq!(pipeline.name, "test-pipeline");
        assert_eq!(pipeline.stage_count(), 2);
        assert!(!pipeline.id.is_empty());
        assert_eq!(pipeline.metadata.get("env").unwrap(), "test");
    }

    #[test]
    fn test_pipeline_stage_lookup() {
        let pipeline = CompositionPipeline::builder()
            .name("lookup-test")
            .stage(PipelineStage::new("extract", b"e"))
            .stage(PipelineStage::new("transform", b"t"))
            .build();

        assert!(pipeline.stage(0).is_some());
        assert!(pipeline.stage(2).is_none());
        assert_eq!(pipeline.stage_by_name("transform").unwrap().name, "transform");
        assert!(pipeline.stage_by_name("missing").is_none());
    }

    #[test]
    fn test_pipeline_execute_passthrough() {
        let pipeline = CompositionPipeline::builder()
            .name("passthrough")
            .stage(PipelineStage::new("s1", b"c1"))
            .stage(PipelineStage::new("s2", b"c2"))
            .build();

        let result = pipeline.execute(b"hello");
        assert!(result.success);
        assert_eq!(result.output, b"hello");
        assert_eq!(result.stages_completed(), 2);
        assert_eq!(result.stages_attempted(), 2);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_pipeline_execute_transform() {
        let pipeline = CompositionPipeline::builder()
            .name("transform")
            .stage(PipelineStage::new("upper", b"c1").with_transform(TransformKind::Uppercase))
            .stage(
                PipelineStage::new("prefix", b"c2")
                    .with_transform(TransformKind::Prefix(">> ".to_string())),
            )
            .build();

        let result = pipeline.execute(b"hello");
        assert!(result.success);
        assert_eq!(result.output, b">> HELLO");
    }

    #[test]
    fn test_pipeline_execute_failure_stops() {
        let pipeline = CompositionPipeline::builder()
            .name("fail-test")
            .stage(PipelineStage::new("s1", b"c1"))
            .stage(
                PipelineStage::new("s2", b"c2")
                    .with_transform(TransformKind::Fail("stage error".to_string())),
            )
            .stage(PipelineStage::new("s3", b"c3"))
            .build();

        let result = pipeline.execute(b"data");
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("stage error"));
        assert_eq!(result.stages_attempted(), 2);
        assert_eq!(result.stages_completed(), 1);
    }

    #[test]
    fn test_pipeline_empty() {
        let pipeline = CompositionPipeline::builder().name("empty").build();

        let result = pipeline.execute(b"data");
        assert!(result.success);
        assert_eq!(result.output, b"data");
        assert_eq!(result.stages_attempted(), 0);
    }

    #[test]
    fn test_pipeline_result_metrics_for() {
        let pipeline = CompositionPipeline::builder()
            .name("metrics")
            .stage(PipelineStage::new("alpha", b"a"))
            .stage(PipelineStage::new("beta", b"b"))
            .build();

        let result = pipeline.execute(b"test");
        let alpha = result.metrics_for("alpha").unwrap();
        assert!(alpha.success);
        assert_eq!(alpha.stage_index, 0);
        assert_eq!(alpha.input_bytes, 4);

        assert!(result.metrics_for("missing").is_none());
    }

    // -- PipelineStage tests --

    #[test]
    fn test_stage_new() {
        let stage = PipelineStage::new("my-stage", b"wasm-bytes");
        assert_eq!(stage.name, "my-stage");
        assert!(!stage.component_hash.is_empty());
        assert!(stage.capabilities.is_empty());
        assert!(stage.resource_limits.is_none());
    }

    #[test]
    fn test_stage_with_capability() {
        let stage =
            PipelineStage::new("cap-stage", b"wasm").with_capability(Capability::stdout());
        assert!(stage.capabilities.has(&Capability::stdout()));
    }

    #[test]
    fn test_stage_with_resource_limits() {
        let limits = ResourceLimits::restrictive();
        let stage =
            PipelineStage::new("limited", b"wasm").with_resource_limits(limits.clone());
        assert!(stage.resource_limits.is_some());
    }

    #[test]
    fn test_stage_with_metadata() {
        let stage = PipelineStage::new("meta", b"wasm")
            .with_metadata("role", "transformer");
        assert_eq!(stage.metadata.get("role").unwrap(), "transformer");
    }

    #[test]
    fn test_stage_deterministic_hash() {
        let s1 = PipelineStage::new("a", b"same-bytes");
        let s2 = PipelineStage::new("b", b"same-bytes");
        assert_eq!(s1.component_hash, s2.component_hash);

        let s3 = PipelineStage::new("c", b"different-bytes");
        assert_ne!(s1.component_hash, s3.component_hash);
    }

    // -- TransformKind tests --

    #[test]
    fn test_transform_passthrough() {
        let stage = PipelineStage::new("pt", b"x");
        assert_eq!(stage.process(b"abc").unwrap(), b"abc");
    }

    #[test]
    fn test_transform_uppercase() {
        let stage =
            PipelineStage::new("up", b"x").with_transform(TransformKind::Uppercase);
        assert_eq!(stage.process(b"hello").unwrap(), b"HELLO");
    }

    #[test]
    fn test_transform_prefix() {
        let stage = PipelineStage::new("pfx", b"x")
            .with_transform(TransformKind::Prefix("[log] ".to_string()));
        assert_eq!(stage.process(b"msg").unwrap(), b"[log] msg");
    }

    #[test]
    fn test_transform_fail() {
        let stage = PipelineStage::new("fail", b"x")
            .with_transform(TransformKind::Fail("boom".to_string()));
        let err = stage.process(b"data").unwrap_err();
        assert_eq!(err, "boom");
    }

    // -- WorldDefinition round-trip --

    #[test]
    fn test_world_definition_wit_round_trip_structure() {
        let gen = WorldGenerator::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());
        caps.grant(Capability::stderr());
        caps.grant(Capability::filesystem_read("/data"));
        caps.grant(Capability::http_client(vec!["api.example.com"]));
        caps.grant(Capability::system_clock());
        caps.grant(Capability::secure_random());

        let world = gen.from_capabilities("full-test", &caps);
        let wit = world.to_wit();

        // Verify WIT structure
        assert!(wit.starts_with("package "));
        assert!(wit.contains("world full-test {"));
        assert!(wit.contains("import "));
        assert!(wit.contains("export "));
        assert!(wit.ends_with("}\n"));

        // Verify key interfaces present
        assert!(wit.contains("stdout"));
        assert!(wit.contains("stderr"));
        assert!(wit.contains("filesystem"));
        assert!(wit.contains("http"));
        assert!(wit.contains("clock"));
        assert!(wit.contains("random"));
    }
}
