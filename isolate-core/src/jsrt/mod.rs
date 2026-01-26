//! Embeddable JavaScript/TypeScript runtime for Isolate.
//!
//! Provides a high-level API for executing JavaScript and TypeScript code
//! in sandboxes without requiring users to compile their own WASM modules.
//! Uses a pre-built JavaScript engine (QuickJS) compiled to WASM.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ JsRuntime                                │
//! │  ┌──────────┐   ┌───────────────────┐   │
//! │  │ JS Source │──▶│ Script Wrapper    │   │
//! │  └──────────┘   │ (adds host binds) │   │
//! │                 └────────┬──────────┘   │
//! │                          ▼              │
//! │                 ┌───────────────────┐   │
//! │                 │ QuickJS WASM      │   │
//! │                 │ (pre-compiled)    │   │
//! │                 └────────┬──────────┘   │
//! │                          ▼              │
//! │                 ┌───────────────────┐   │
//! │                 │ Isolate Sandbox   │   │
//! │                 └───────────────────┘   │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use isolate_core::jsrt::{
//!     JsRuntime, JsRuntimeConfig, JsRequest, JsResult,
//! };
//!
//! let runtime = JsRuntime::new(JsRuntimeConfig::default());
//!
//! let request = JsRequest::new("console.log('Hello from JS!');");
//! let checks = runtime.validate(&request);
//! assert!(checks.is_valid());
//! ```

// This module is experimental and not all APIs are used yet.
#![allow(dead_code)]

mod runtime;
pub mod transpiler;

pub use runtime::{
    HostBinding, HostBindingType, JsRequest, JsResult, JsRuntime, JsRuntimeConfig, JsValidation,
    TranspileConfig,
};
pub use transpiler::{TranspileResult, TsTranspiler};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let config = JsRuntimeConfig::default();
        assert!(config.enable_console);
        let runtime = JsRuntime::new(config);
        assert_eq!(runtime.stats().total_executions, 0);
    }
}
