//! WASM execution engine.
//!
//! This module provides the core WASM execution capabilities using Wasmtime.

mod capture;
mod host;
mod wasm;

pub use capture::{new_capture_buffer, CaptureBuffer, CaptureStream, NullStream};
pub use host::{HostFunctions, HostState};
pub use wasm::{CompiledModule, WasmEngine, WasmInstance};
