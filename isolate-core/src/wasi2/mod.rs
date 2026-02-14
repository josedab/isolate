//! WASI Preview2 (WASIp2) support for the Isolate sandbox runtime.
//!
//! This module provides support for running WebAssembly Components using the
//! WASI Preview2 specification, which includes:
//!
//! - Component Model support for modular WebAssembly
//! - Stream-based I/O with wasi-io
//! - Enhanced filesystem access with wasi-filesystem
//! - Network support with wasi-sockets
//! - HTTP client/server with wasi-http
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::wasi2::{ComponentSandbox, ComponentConfig};
//!
//! # async fn example() -> isolate_core::Result<()> {
//! // Load a WASM component
//! let component_bytes = std::fs::read("component.wasm")?;
//!
//! // Configure the component sandbox
//! let config = ComponentConfig::builder()
//!     .component(&component_bytes)?
//!     .memory_limit(128 * 1024 * 1024)
//!     .allow_stdout()
//!     .allow_stderr()
//!     .build()?;
//!
//! // Create and run the component
//! let mut sandbox = ComponentSandbox::create(config).await?;
//! let output = sandbox.run().await?;
//!
//! println!("Exit code: {}", output.exit_code);
//! # Ok(())
//! # }
//! ```

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

pub mod async_io;
pub mod capability_bridge;
mod component;
pub mod composition;
mod context;
pub mod dual_mode;
mod host;
pub mod migration;
pub mod production;
pub mod readiness;
pub mod resource_handles;
pub mod runtime;
pub mod compat_shim;
pub mod interface_registry;
pub mod wit;
pub mod wit_parser;
pub mod world_gen;

pub use component::{
    CompiledComponent, ComponentEngine, ComponentEngineConfig, ComponentSandbox, ComponentState,
};
pub use context::{
    ComponentConfig, ComponentConfigBuilder, ComponentHash, NetworkConfig, WasmComponent,
};
pub use host::{IoLimits, WasiError, WasiHostState};
pub use dual_mode::{detect_wasi_version, DualModeSandbox, WasiVersion};
pub use interface_registry::{
    CapabilityRef, InterfaceBinding, InterfaceRegistry, WorldDefinition,
};
pub use world_gen::{
    WorldGenerator, WorldDefinition as WitWorldDefinition, CompositionPipeline, PipelineStage,
    PipelineResult,
};

use crate::Result;

/// Check if a byte slice contains a WASM component (vs a module).
///
/// Components have a different header structure than modules.
pub fn is_component(bytes: &[u8]) -> bool {
    // WASM components use the same magic number but have a layer section
    // indicating they are components. The component model uses a "layer"
    // byte at position 8 with value > 0 for components.
    if bytes.len() < 12 {
        return false;
    }

    // Check WASM magic number
    if &bytes[0..4] != b"\0asm" {
        return false;
    }

    // Check for component layer (version section indicates component)
    // Components typically have a specific version indicator
    // For now, we use a simple heuristic based on the binary structure

    // Look for component section indicator
    // In practice, you'd parse the binary more carefully
    bytes[4..8] != [0x01, 0x00, 0x00, 0x00]
}

/// Validate a WASM component.
pub fn validate_component(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 8 {
        return Err(crate::Error::ModuleValidation("Component too small".to_string()));
    }

    if &bytes[0..4] != b"\0asm" {
        return Err(crate::Error::ModuleValidation("Invalid WASM magic number".to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module
    const MINIMAL_MODULE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version 1 (module)
    ];

    #[test]
    fn test_is_component() {
        // Regular module should return false
        assert!(!is_component(MINIMAL_MODULE));

        // Too small should return false
        assert!(!is_component(&[0x00, 0x61]));

        // Invalid magic should return false
        assert!(!is_component(&[
            0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
        ]));
    }

    #[test]
    fn test_validate_component() {
        // Valid module passes validation
        assert!(validate_component(MINIMAL_MODULE).is_ok());

        // Too small fails
        assert!(validate_component(&[0x00, 0x61]).is_err());

        // Invalid magic fails
        assert!(validate_component(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]).is_err());
    }
}
