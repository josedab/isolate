//! WASM execution engine.
//!
//! This module provides the core WASM execution capabilities using Wasmtime.

mod capture;
pub mod event_stream;
mod host;
pub mod host_sdk;
pub mod multi_tenant;
pub mod plugin_api;
pub mod pre_initialized;
pub mod registry;
pub mod streaming;
pub mod triggers;
pub mod scheduler;
pub mod toolchain;
mod wasm;

pub use capture::{
    new_capture_buffer, CaptureBuffer, CaptureStream, NullStream, OutputChunk, OutputSource,
    StreamingCaptureStream,
};
pub use host::{HostFunctions, HostState};
pub use pre_initialized::{PreInitConfig, PreInitStats, PreInitializedPool};
pub use streaming::{
    channel, ChannelError, GuestHalf, HostHalf, RingReader, RingWriter, StreamingChannel,
};
pub use wasm::{CompiledModule, WasmEngine, WasmInstance};
