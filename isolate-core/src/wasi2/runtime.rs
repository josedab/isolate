//! Unified runtime that auto-detects WASM module vs component and executes accordingly.
//!
//! Provides a single entry point for executing any WASM binary, automatically
//! routing to Preview1 (module) or Preview2 (component) execution paths.
//!
//! ```rust,ignore
//! use isolate_core::wasi2::runtime::{UnifiedRuntime, ExecutionMode};
//!
//! let runtime = UnifiedRuntime::new()?;
//! let mode = runtime.detect(wasm_bytes);
//! let output = runtime.execute(wasm_bytes, config).await?;
//! ```

use super::component::{ComponentEngine, ComponentSandbox};
use super::context::ComponentConfig;
use super::dual_mode::WasiVersion;
use crate::config::ModuleHash;
use crate::error::Result;

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Detected execution mode for a WASM binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// WASI Preview1 module.
    Preview1,
    /// WASI Preview2 component.
    Preview2,
    /// Unknown format.
    Unknown,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview1 => write!(f, "WASI Preview1 (Module)"),
            Self::Preview2 => write!(f, "WASI Preview2 (Component)"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Result of executing a WASM binary through the unified runtime.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// The detected execution mode.
    pub mode: ExecutionMode,
    /// Execution output.
    pub exit_code: i32,
    /// Stdout bytes.
    pub stdout: Vec<u8>,
    /// Stderr bytes.
    pub stderr: Vec<u8>,
    /// Total wall time.
    pub wall_time: Duration,
    /// Fuel consumed (if metered).
    pub fuel_consumed: u64,
}

/// Metadata about a WASM binary detected by the runtime.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Detected execution mode.
    pub mode: ExecutionMode,
    /// SHA-256 hash.
    pub hash: String,
    /// Binary size in bytes.
    pub size: usize,
    /// Whether the binary appears valid.
    pub valid: bool,
    /// Detected exports (if parseable).
    pub exports: Vec<String>,
}

/// Statistics from the unified runtime.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStats {
    /// Total executions.
    pub total_executions: u64,
    /// Preview1 executions.
    pub preview1_executions: u64,
    /// Preview2 executions.
    pub preview2_executions: u64,
    /// Failed executions.
    pub failed_executions: u64,
    /// Total fuel consumed.
    pub total_fuel_consumed: u64,
    /// Average execution time.
    pub avg_execution_ms: f64,
}

/// Unified runtime that handles both Preview1 modules and Preview2 components.
pub struct UnifiedRuntime {
    /// Component engine for Preview2.
    component_engine: ComponentEngine,
    /// Execution statistics.
    stats: RuntimeStats,
    /// Module info cache.
    info_cache: HashMap<String, ModuleInfo>,
}

impl UnifiedRuntime {
    /// Create a new unified runtime.
    pub fn new() -> Result<Self> {
        let component_engine = ComponentEngine::new()?;
        Ok(Self { component_engine, stats: RuntimeStats::default(), info_cache: HashMap::new() })
    }

    /// Detect the execution mode of a WASM binary.
    pub fn detect(&self, bytes: &[u8]) -> ExecutionMode {
        if bytes.len() < 8 {
            return ExecutionMode::Unknown;
        }

        // Check WASM magic number
        if &bytes[0..4] != b"\0asm" {
            return ExecutionMode::Unknown;
        }

        // Use dual_mode detection
        match super::dual_mode::detect_wasi_version(bytes) {
            WasiVersion::Preview1 => ExecutionMode::Preview1,
            WasiVersion::Preview2 => ExecutionMode::Preview2,
            WasiVersion::Unknown => {
                // Fallback: check version field
                if bytes[4..8] == [0x01, 0x00, 0x00, 0x00] {
                    ExecutionMode::Preview1
                } else {
                    ExecutionMode::Preview2
                }
            }
        }
    }

    /// Analyze a WASM binary and return metadata.
    pub fn analyze(&mut self, bytes: &[u8]) -> ModuleInfo {
        let mode = self.detect(bytes);
        let hash = ModuleHash::from_bytes(bytes);
        let hash_str = hash.0.clone();

        // Check cache
        if let Some(cached) = self.info_cache.get(&hash_str) {
            return cached.clone();
        }

        let valid = bytes.len() >= 8 && &bytes[0..4] == b"\0asm";

        let info = ModuleInfo {
            mode,
            hash: hash_str.clone(),
            size: bytes.len(),
            valid,
            exports: Vec::new(), // Would need full parse to extract
        };

        self.info_cache.insert(hash_str, info.clone());
        info
    }

    /// Execute a Preview2 component.
    pub async fn execute_component(
        &mut self,
        _bytes: &[u8],
        config: ComponentConfig,
    ) -> Result<ExecutionResult> {
        let start = Instant::now();

        let engine = self.component_engine.engine();
        let mut sandbox = ComponentSandbox::create_with_engine(config, engine).await?;
        let output = sandbox.run(&[]).await?;

        let result = ExecutionResult {
            mode: ExecutionMode::Preview2,
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            wall_time: start.elapsed(),
            fuel_consumed: output.resource_usage.fuel_consumed,
        };

        self.stats.total_executions += 1;
        self.stats.preview2_executions += 1;
        self.stats.total_fuel_consumed += result.fuel_consumed;

        Ok(result)
    }

    /// Get runtime statistics.
    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }

    /// Get the component engine for direct use.
    pub fn component_engine(&self) -> &ComponentEngine {
        &self.component_engine
    }

    /// Clear all caches.
    pub fn clear_caches(&mut self) {
        self.component_engine.clear_cache();
        self.info_cache.clear();
    }

    /// Record a failed execution.
    pub fn record_failure(&mut self) {
        self.stats.total_executions += 1;
        self.stats.failed_executions += 1;
    }

    /// Record a Preview1 execution (called by external executor).
    pub fn record_preview1_execution(&mut self, fuel_consumed: u64) {
        self.stats.total_executions += 1;
        self.stats.preview1_executions += 1;
        self.stats.total_fuel_consumed += fuel_consumed;
    }
}

/// Capability requirement detected from a component's imports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequiredCapability {
    Stdout,
    Stderr,
    Stdin,
    FilesystemRead,
    FilesystemWrite,
    NetworkOutbound,
    NetworkInbound,
    EnvironmentVars,
    Clock,
    Random,
    HttpClient,
}

impl std::fmt::Display for RequiredCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdout => write!(f, "stdout"),
            Self::Stderr => write!(f, "stderr"),
            Self::Stdin => write!(f, "stdin"),
            Self::FilesystemRead => write!(f, "filesystem:read"),
            Self::FilesystemWrite => write!(f, "filesystem:write"),
            Self::NetworkOutbound => write!(f, "network:outbound"),
            Self::NetworkInbound => write!(f, "network:inbound"),
            Self::EnvironmentVars => write!(f, "env"),
            Self::Clock => write!(f, "clock"),
            Self::Random => write!(f, "random"),
            Self::HttpClient => write!(f, "http:client"),
        }
    }
}

/// Infer required capabilities from WASI interface names.
pub fn infer_capabilities(interface_names: &[&str]) -> Vec<RequiredCapability> {
    let mut caps = Vec::new();

    for name in interface_names {
        match *name {
            n if n.contains("stdout") => caps.push(RequiredCapability::Stdout),
            n if n.contains("stderr") => caps.push(RequiredCapability::Stderr),
            n if n.contains("stdin") => caps.push(RequiredCapability::Stdin),
            n if n.contains("filesystem") && n.contains("read") => {
                caps.push(RequiredCapability::FilesystemRead)
            }
            n if n.contains("filesystem") && n.contains("write") => {
                caps.push(RequiredCapability::FilesystemWrite)
            }
            n if n.contains("filesystem") => caps.push(RequiredCapability::FilesystemRead),
            n if n.contains("tcp") || n.contains("udp") || n.contains("socket") => {
                caps.push(RequiredCapability::NetworkOutbound)
            }
            n if n.contains("http") && n.contains("outgoing") => {
                caps.push(RequiredCapability::HttpClient)
            }
            n if n.contains("http") => caps.push(RequiredCapability::HttpClient),
            n if n.contains("environment") || n.contains("env") => {
                caps.push(RequiredCapability::EnvironmentVars)
            }
            n if n.contains("clock") || n.contains("monotonic") || n.contains("wall") => {
                caps.push(RequiredCapability::Clock)
            }
            n if n.contains("random") => caps.push(RequiredCapability::Random),
            _ => {}
        }
    }

    caps.sort_by_key(|c| format!("{}", c));
    caps.dedup();
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_MODULE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version 1 (module)
    ];

    const COMPONENT_HEADER: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x0d, 0x00, 0x01, 0x00, // version (component)
        0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn test_detect_preview1() {
        let runtime = UnifiedRuntime::new().unwrap();
        assert_eq!(runtime.detect(MINIMAL_MODULE), ExecutionMode::Preview1);
    }

    #[test]
    fn test_detect_component() {
        let runtime = UnifiedRuntime::new().unwrap();
        assert_eq!(runtime.detect(COMPONENT_HEADER), ExecutionMode::Preview2);
    }

    #[test]
    fn test_detect_unknown() {
        let runtime = UnifiedRuntime::new().unwrap();
        assert_eq!(runtime.detect(&[0x00, 0x01]), ExecutionMode::Unknown);
        assert_eq!(runtime.detect(&[0xFF; 12]), ExecutionMode::Unknown);
    }

    #[test]
    fn test_analyze_module() {
        let mut runtime = UnifiedRuntime::new().unwrap();
        let info = runtime.analyze(MINIMAL_MODULE);

        assert_eq!(info.mode, ExecutionMode::Preview1);
        assert_eq!(info.size, 8);
        assert!(info.valid);
    }

    #[test]
    fn test_analyze_caches() {
        let mut runtime = UnifiedRuntime::new().unwrap();
        let info1 = runtime.analyze(MINIMAL_MODULE);
        let info2 = runtime.analyze(MINIMAL_MODULE);

        assert_eq!(info1.hash, info2.hash);
    }

    #[test]
    fn test_execution_mode_display() {
        assert_eq!(ExecutionMode::Preview1.to_string(), "WASI Preview1 (Module)");
        assert_eq!(ExecutionMode::Preview2.to_string(), "WASI Preview2 (Component)");
    }

    #[test]
    fn test_stats_initial() {
        let runtime = UnifiedRuntime::new().unwrap();
        let stats = runtime.stats();
        assert_eq!(stats.total_executions, 0);
        assert_eq!(stats.preview1_executions, 0);
        assert_eq!(stats.preview2_executions, 0);
    }

    #[test]
    fn test_record_preview1() {
        let mut runtime = UnifiedRuntime::new().unwrap();
        runtime.record_preview1_execution(1000);
        runtime.record_preview1_execution(2000);

        assert_eq!(runtime.stats().total_executions, 2);
        assert_eq!(runtime.stats().preview1_executions, 2);
        assert_eq!(runtime.stats().total_fuel_consumed, 3000);
    }

    #[test]
    fn test_record_failure() {
        let mut runtime = UnifiedRuntime::new().unwrap();
        runtime.record_failure();

        assert_eq!(runtime.stats().total_executions, 1);
        assert_eq!(runtime.stats().failed_executions, 1);
    }

    #[test]
    fn test_clear_caches() {
        let mut runtime = UnifiedRuntime::new().unwrap();
        runtime.analyze(MINIMAL_MODULE);
        assert!(!runtime.info_cache.is_empty());

        runtime.clear_caches();
        assert!(runtime.info_cache.is_empty());
    }

    #[test]
    fn test_infer_capabilities_filesystem() {
        let caps = infer_capabilities(&["wasi:filesystem/read"]);
        assert!(caps.contains(&RequiredCapability::FilesystemRead));
    }

    #[test]
    fn test_infer_capabilities_network() {
        let caps = infer_capabilities(&["wasi:sockets/tcp", "wasi:http/outgoing-handler"]);
        assert!(caps.contains(&RequiredCapability::NetworkOutbound));
        assert!(caps.contains(&RequiredCapability::HttpClient));
    }

    #[test]
    fn test_infer_capabilities_mixed() {
        let caps = infer_capabilities(&[
            "wasi:cli/stdout",
            "wasi:cli/stderr",
            "wasi:clocks/wall-clock",
            "wasi:random/random",
            "wasi:cli/environment",
        ]);
        assert!(caps.contains(&RequiredCapability::Stdout));
        assert!(caps.contains(&RequiredCapability::Stderr));
        assert!(caps.contains(&RequiredCapability::Clock));
        assert!(caps.contains(&RequiredCapability::Random));
        assert!(caps.contains(&RequiredCapability::EnvironmentVars));
    }

    #[test]
    fn test_infer_capabilities_empty() {
        let caps = infer_capabilities(&[]);
        assert!(caps.is_empty());
    }

    #[test]
    fn test_required_capability_display() {
        assert_eq!(RequiredCapability::Stdout.to_string(), "stdout");
        assert_eq!(RequiredCapability::FilesystemRead.to_string(), "filesystem:read");
        assert_eq!(RequiredCapability::NetworkOutbound.to_string(), "network:outbound");
    }
}
