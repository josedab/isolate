//! Library interface for isolate-server.
//!
//! This module re-exports the service implementation and proto types
//! to allow integration testing and embedding.

/// Generated protobuf types.
pub mod proto {
    tonic::include_proto!("isolate.v1");
}

/// gRPC service implementation.
pub mod service;

/// Authentication interceptor.
pub mod auth;
