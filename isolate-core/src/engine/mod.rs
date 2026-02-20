//! WASM execution engine.
//!
//! This module provides the core WASM execution capabilities for Isolate,
//! built on top of the [Wasmtime](https://wasmtime.dev/) runtime. It handles
//! module compilation, instantiation, WASI configuration, I/O capture, and
//! host function registration.
//!
//! # Architecture
//!
//! The engine is organized into several layers:
//!
//! - **Core execution** ([`WasmEngine`], [`WasmInstance`], [`CompiledModule`]):
//!   Module compilation with caching, instance creation with WASI setup, and
//!   epoch-based timeout interruption.
//! - **I/O capture** ([`CaptureStream`], [`CaptureBuffer`], [`NullStream`]):
//!   Capture stdout/stderr output from sandboxed modules, with support for
//!   streaming and null sinks.
//! - **Host functions** ([`HostFunctions`], [`HostState`]): Registry for
//!   injecting host-side functions callable from WASM guest code.
//! - **Streaming channels** ([`StreamingChannel`], [`RingWriter`], [`RingReader`]):
//!   Lock-free ring buffer channels for bidirectional host↔guest communication.
//! - **Pre-initialization** ([`PreInitializedPool`]): Pool of pre-compiled and
//!   pre-instantiated modules for sub-millisecond warm starts.
//!
//! # Public Submodules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`event_stream`] | Event-driven stream processing for sandbox lifecycle events |
//! | [`host_sdk`] | SDK for defining custom host functions exposed to WASM guests |
//! | [`multi_tenant`] | Multi-tenant engine sharing with per-tenant isolation |
//! | [`plugin_api`] | Plugin API for extending engine capabilities |
//! | [`pre_initialized`] | Pre-warmed module pool for fast instantiation |
//! | [`registry`] | Module registry for caching and sharing compiled modules |
//! | [`streaming`] | Ring buffer channels for host↔guest streaming I/O |
//! | [`triggers`] | Event-based triggers for sandbox lifecycle hooks |
//! | [`scheduler`] | Execution scheduler for managing concurrent sandbox runs |
//! | [`toolchain`] | Toolchain integration for compiling source to WASM |
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::engine::{WasmEngine, CaptureStream, NullStream};
//!
//! // Create a shared engine (caches compiled modules)
//! let engine = WasmEngine::new().expect("engine creation");
//! ```

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
