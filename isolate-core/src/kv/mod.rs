//! Embedded Key-Value Store for sandboxed state management.
//!
//! Provides per-sandbox namespaced key-value storage with TTL support,
//! size limits, and optional persistence.
//!
//! # Features
//!
//! - **Namespace Isolation**: Each sandbox gets its own isolated namespace
//! - **TTL Support**: Keys can have time-to-live expiration
//! - **Size Limits**: Per-namespace storage quotas
//! - **Atomic Operations**: Compare-and-swap for safe concurrent access
//! - **Persistence**: Optional disk-backed storage with configurable durability
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::kv::{KvStore, KvConfig, Namespace};
//!
//! let store = KvStore::new(KvConfig::default());
//! let ns = store.namespace("sandbox-123");
//!
//! ns.set("key", b"value", None)?;
//! let value = ns.get("key")?;
//! assert_eq!(value.unwrap().data(), b"value");
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.


mod store;
pub mod replication;

pub use store::{
    Entry, KvConfig, KvError, KvStore, Namespace, NamespaceId, NamespaceStats, SetOptions,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let store = KvStore::new(KvConfig::default());
        let ns = store.namespace("test");
        assert_eq!(ns.id().as_str(), "test");
    }
}
