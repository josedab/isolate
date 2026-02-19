//! Sandbox networking with policy engine.
//!
//! Provides controlled network access from sandboxes with a declarative policy
//! engine for fine-grained access control. All network operations are
//! host-mediated — sandboxes never get raw socket access.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
//! │   Sandbox    │────▶│ Policy Engine │────▶│ Host Network │
//! │  (WASM code) │     │  (evaluate)   │     │   (execute)  │
//! └──────────────┘     └──────────────┘     └──────────────┘
//! ```
//!
//! The policy engine evaluates each network operation against a set of rules
//! before allowing or denying the request. All decisions are audit-logged.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::network::policy::{NetworkPolicy, PolicyRule, PolicyAction};
//!
//! let policy = NetworkPolicy::builder()
//!     .allow_http("*.api.example.com")
//!     .allow_http("cdn.trusted.com")
//!     .deny_cidr("10.0.0.0/8")
//!     .max_connections(10)
//!     .rate_limit(100, std::time::Duration::from_secs(60))
//!     .require_tls(true)
//!     .build();
//!
//! assert!(policy.allows_http_host("sub.api.example.com"));
//! assert!(!policy.allows_http_host("evil.com"));
//! ```



pub mod dns;
pub mod policy;
pub mod tcp;
pub mod zero_trust;

pub use dns::DnsResolver;
pub use policy::{NetworkPolicy, NetworkPolicyBuilder, PolicyAction, PolicyRule};
pub use tcp::{TcpConnectionConfig, TcpConnectionPool};
