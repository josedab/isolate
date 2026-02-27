//! Resource handle management for WASI Preview 2 Component Model.
//!
//! Provides lifecycle management for Component Model resource handles including
//! creation, borrowing, ownership transfer, and automatic cleanup.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Unique identifier for a resource handle.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceHandle(u32);

impl ResourceHandle {
    pub fn id(&self) -> u32 {
        self.0
    }
}

/// Ownership model for a resource handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ownership {
    /// The handle is owned; dropping it will destroy the resource.
    Owned,
    /// The handle is borrowed; the resource outlives this reference.
    Borrowed,
}

/// Metadata about a resource handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub handle: ResourceHandle,
    pub resource_type: String,
    pub ownership: Ownership,
    pub created_epoch_ms: u64,
    pub ref_count: u32,
}

/// Table managing resource handle lifecycles within a component instance.
///
/// Ensures handles are tracked, ref-counted, and cleaned up on drop.
pub struct ResourceTable {
    next_handle: AtomicU32,
    entries: parking_lot::RwLock<HashMap<ResourceHandle, ResourceEntry>>,
    total_created: AtomicU64,
    total_dropped: AtomicU64,
}

struct ResourceEntry {
    resource_type: String,
    ownership: Ownership,
    created_epoch_ms: u64,
    ref_count: u32,
}

impl ResourceTable {
    pub fn new() -> Self {
        Self {
            next_handle: AtomicU32::new(1),
            entries: parking_lot::RwLock::new(HashMap::new()),
            total_created: AtomicU64::new(0),
            total_dropped: AtomicU64::new(0),
        }
    }

    /// Create a new owned resource handle.
    pub fn create(&self, resource_type: impl Into<String>) -> ResourceHandle {
        let handle = ResourceHandle(self.next_handle.fetch_add(1, Ordering::Relaxed));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.entries.write().insert(
            handle,
            ResourceEntry {
                resource_type: resource_type.into(),
                ownership: Ownership::Owned,
                created_epoch_ms: now,
                ref_count: 1,
            },
        );
        self.total_created.fetch_add(1, Ordering::Relaxed);
        handle
    }

    /// Borrow an existing resource handle (increments ref count).
    pub fn borrow(&self, handle: ResourceHandle) -> Option<ResourceHandle> {
        let mut entries = self.entries.write();
        let entry = entries.get_mut(&handle)?;
        entry.ref_count += 1;

        // Create a borrowed alias handle
        let borrowed_handle = ResourceHandle(self.next_handle.fetch_add(1, Ordering::Relaxed));
        let borrowed_entry = ResourceEntry {
            resource_type: entry.resource_type.clone(),
            ownership: Ownership::Borrowed,
            created_epoch_ms: entry.created_epoch_ms,
            ref_count: 0, // borrowed handles don't own refs
        };
        entries.insert(borrowed_handle, borrowed_entry);
        Some(borrowed_handle)
    }

    /// Drop a resource handle. If owned and ref_count reaches 0, resource is destroyed.
    pub fn drop_handle(&self, handle: ResourceHandle) -> bool {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&handle) {
            if entry.ownership == Ownership::Owned && entry.ref_count > 0 {
                entry.ref_count -= 1;
                if entry.ref_count == 0 {
                    entries.remove(&handle);
                    self.total_dropped.fetch_add(1, Ordering::Relaxed);
                    return true; // resource destroyed
                }
            } else {
                // Borrowed handles just get removed from the table
                entries.remove(&handle);
            }
        }
        false
    }

    /// Get info about a resource handle.
    pub fn get_info(&self, handle: ResourceHandle) -> Option<ResourceInfo> {
        self.entries.read().get(&handle).map(|e| ResourceInfo {
            handle,
            resource_type: e.resource_type.clone(),
            ownership: e.ownership,
            created_epoch_ms: e.created_epoch_ms,
            ref_count: e.ref_count,
        })
    }

    /// Number of active handles.
    pub fn active_count(&self) -> usize {
        self.entries.read().len()
    }

    /// Total handles created since table creation.
    pub fn total_created(&self) -> u64 {
        self.total_created.load(Ordering::Relaxed)
    }

    /// Total handles dropped since table creation.
    pub fn total_dropped(&self) -> u64 {
        self.total_dropped.load(Ordering::Relaxed)
    }

    /// Drop all handles (cleanup on component termination).
    pub fn clear(&self) {
        let mut entries = self.entries.write();
        let count = entries.len() as u64;
        entries.clear();
        self.total_dropped.fetch_add(count, Ordering::Relaxed);
    }
}

impl Default for ResourceTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_handle() {
        let table = ResourceTable::new();
        let h = table.create("file-descriptor");
        assert_eq!(h.id(), 1);
        assert_eq!(table.active_count(), 1);
        assert_eq!(table.total_created(), 1);
    }

    #[test]
    fn test_create_multiple() {
        let table = ResourceTable::new();
        let h1 = table.create("fd");
        let h2 = table.create("socket");
        assert_ne!(h1.id(), h2.id());
        assert_eq!(table.active_count(), 2);
    }

    #[test]
    fn test_borrow_handle() {
        let table = ResourceTable::new();
        let owned = table.create("stream");
        let borrowed = table.borrow(owned).unwrap();

        let info = table.get_info(borrowed).unwrap();
        assert_eq!(info.ownership, Ownership::Borrowed);
        assert_eq!(table.active_count(), 2);

        // Original should have incremented ref count
        let orig_info = table.get_info(owned).unwrap();
        assert_eq!(orig_info.ref_count, 2);
    }

    #[test]
    fn test_drop_owned_handle() {
        let table = ResourceTable::new();
        let h = table.create("resource");
        let destroyed = table.drop_handle(h);
        assert!(destroyed);
        assert_eq!(table.active_count(), 0);
        assert_eq!(table.total_dropped(), 1);
    }

    #[test]
    fn test_drop_borrowed_handle() {
        let table = ResourceTable::new();
        let owned = table.create("resource");
        let borrowed = table.borrow(owned).unwrap();

        // Dropping borrowed handle shouldn't destroy the resource
        table.drop_handle(borrowed);
        assert!(table.get_info(owned).is_some()); // owned still exists
    }

    #[test]
    fn test_borrow_nonexistent() {
        let table = ResourceTable::new();
        assert!(table.borrow(ResourceHandle(999)).is_none());
    }

    #[test]
    fn test_get_info() {
        let table = ResourceTable::new();
        let h = table.create("my-type");

        let info = table.get_info(h).unwrap();
        assert_eq!(info.resource_type, "my-type");
        assert_eq!(info.ownership, Ownership::Owned);
        assert_eq!(info.ref_count, 1);
    }

    #[test]
    fn test_clear() {
        let table = ResourceTable::new();
        table.create("a");
        table.create("b");
        table.create("c");
        assert_eq!(table.active_count(), 3);

        table.clear();
        assert_eq!(table.active_count(), 0);
        assert_eq!(table.total_dropped(), 3);
    }

    #[test]
    fn test_missing_info() {
        let table = ResourceTable::new();
        assert!(table.get_info(ResourceHandle(42)).is_none());
    }
}
