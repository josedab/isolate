//! WASM execution engine.
//!
//! This module provides the core WASM execution capabilities using Wasmtime.

mod capture;
mod host;
pub mod streaming;
mod wasm;

pub use capture::{new_capture_buffer, CaptureBuffer, CaptureStream, NullStream};
pub use host::{HostFunctions, HostState};
pub use streaming::{
    channel, ChannelError, GuestHalf, HostHalf, RingReader, RingWriter, StreamingChannel,
};
pub use wasm::{CompiledModule, WasmEngine, WasmInstance};
