//! Virtual Filesystem for sandbox isolation.
//!
//! Provides a layered virtual filesystem that sandboxes can use instead of
//! accessing the real filesystem. Supports overlay mounts, quota enforcement,
//! and capability-based access control.



#![allow(missing_docs)]
mod layer;
mod quota;

pub use layer::{VfsLayer, VfsNode, VfsPermissions, VfsStat, VirtualFilesystem};
pub use quota::{QuotaUsage, VfsQuota, VfsQuotaTracker};
