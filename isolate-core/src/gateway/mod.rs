//! HTTP/REST Gateway for sandbox management.
//!
//! Provides a RESTful API layer for sandbox operations, complementing
//! the gRPC server with an HTTP-accessible interface.
//!
//! # Features
//!
//! - **RESTful API**: Standard CRUD operations for sandboxes
//! - **SSE Streaming**: Server-sent events for real-time output
//! - **Rate Limiting**: Per-client request throttling
//! - **API Key Auth**: Simple API key authentication
//! - **OpenAPI Spec**: Auto-generated API documentation
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | /api/v1/sandboxes | Create a sandbox |
//! | GET | /api/v1/sandboxes/:id | Get sandbox status |
//! | POST | /api/v1/sandboxes/:id/run | Execute a sandbox |
//! | DELETE | /api/v1/sandboxes/:id | Terminate a sandbox |
//! | GET | /api/v1/sandboxes | List sandboxes |
//! | GET | /api/v1/metrics | Get metrics |
//! | GET | /api/v1/health | Health check |

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.

pub mod http_handler;
pub mod namespace;
mod router;
mod types;
pub mod websocket;

pub use namespace::{
    IsolationLevel, Namespace, NamespaceAuditEntry, NamespaceId, NamespaceManager, NamespaceStatus,
    QuotaCheckResult, TenantQuota, UsageSnapshot,
};
pub use router::{GatewayConfig, GatewayRouter, RateLimitConfig, Route, RouteHandler};
pub use types::{
    ApiError, ApiResponse, CreateSandboxRequest, ListSandboxesResponse, RunSandboxRequest,
    RunSandboxResponse, SandboxInfo,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let config = GatewayConfig::default();
        assert_eq!(config.prefix, "/api/v1");
    }
}
