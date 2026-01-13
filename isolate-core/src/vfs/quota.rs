//! Filesystem quota enforcement.
//!
//! Provides quota tracking for virtual filesystems, enforcing limits on
//! total bytes, file count, individual file size, and path depth. This
//! prevents sandboxed code from exhausting host storage resources.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Filesystem quota configuration.
///
/// Defines the maximum resource usage allowed for a virtual filesystem.
/// A value of `0` for any limit means that limit is not enforced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsQuota {
    /// Maximum total bytes across all files.
    pub max_total_bytes: u64,
    /// Maximum number of files (including directories).
    pub max_file_count: u64,
    /// Maximum size of a single file in bytes.
    pub max_file_size: u64,
    /// Maximum depth of nested directories.
    pub max_path_depth: u32,
}

impl VfsQuota {
    /// Create a restrictive quota suitable for untrusted code.
    pub fn restrictive() -> Self {
        Self {
            max_total_bytes: 10 * 1024 * 1024, // 10 MB
            max_file_count: 100,
            max_file_size: 1024 * 1024, // 1 MB
            max_path_depth: 10,
        }
    }

    /// Create a permissive quota for trusted code.
    pub fn permissive() -> Self {
        Self {
            max_total_bytes: 1024 * 1024 * 1024, // 1 GB
            max_file_count: 100_000,
            max_file_size: 100 * 1024 * 1024, // 100 MB
            max_path_depth: 50,
        }
    }

    /// Create an unlimited quota (no enforcement).
    pub fn unlimited() -> Self {
        Self { max_total_bytes: 0, max_file_count: 0, max_file_size: 0, max_path_depth: 0 }
    }
}

impl Default for VfsQuota {
    fn default() -> Self {
        Self::restrictive()
    }
}

/// Tracks current filesystem usage and enforces quota limits.
///
/// The tracker uses atomic operations for counters, making it safe to
/// share across threads without external locking.
///
/// # Example
///
/// ```rust
/// use isolate_core::vfs::{VfsQuota, VfsQuotaTracker};
///
/// let quota = VfsQuota {
///     max_total_bytes: 1024,
///     max_file_count: 10,
///     max_file_size: 512,
///     max_path_depth: 5,
/// };
///
/// let tracker = VfsQuotaTracker::new(quota);
///
/// // Check before writing
/// tracker.check_write(256).unwrap();
/// tracker.record_write(256);
/// tracker.record_create();
///
/// let usage = tracker.usage();
/// assert_eq!(usage.current_bytes, 256);
/// assert_eq!(usage.file_count, 1);
/// ```
pub struct VfsQuotaTracker {
    inner: Arc<VfsQuotaTrackerInner>,
}

struct VfsQuotaTrackerInner {
    /// The quota limits being enforced.
    quota: VfsQuota,
    /// Current total bytes used across all files.
    current_bytes: AtomicU64,
    /// Current number of files and directories.
    file_count: AtomicU64,
}

impl std::fmt::Debug for VfsQuotaTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfsQuotaTracker")
            .field("quota", &self.inner.quota)
            .field("current_bytes", &self.inner.current_bytes.load(Ordering::Relaxed))
            .field("file_count", &self.inner.file_count.load(Ordering::Relaxed))
            .finish()
    }
}

impl VfsQuotaTracker {
    /// Create a new quota tracker with the given quota limits.
    pub fn new(quota: VfsQuota) -> Self {
        Self {
            inner: Arc::new(VfsQuotaTrackerInner {
                quota,
                current_bytes: AtomicU64::new(0),
                file_count: AtomicU64::new(0),
            }),
        }
    }

    /// Check if a write of the given size is allowed under the quota.
    ///
    /// This performs a read-only check and does not modify the tracker state.
    /// Call [`record_write`](Self::record_write) after a successful write.
    pub fn check_write(&self, size: u64) -> Result<()> {
        let quota = &self.inner.quota;

        // Check individual file size limit.
        if quota.max_file_size > 0 && size > quota.max_file_size {
            return Err(Error::Execution(format!(
                "VFS quota: file size {} exceeds maximum allowed file size {}",
                size, quota.max_file_size
            )));
        }

        // Check total bytes limit.
        if quota.max_total_bytes > 0 {
            let current = self.inner.current_bytes.load(Ordering::Relaxed);
            let new_total = current.saturating_add(size);
            if new_total > quota.max_total_bytes {
                return Err(Error::Execution(format!(
                    "VFS quota: total bytes {} would exceed limit {}",
                    new_total, quota.max_total_bytes
                )));
            }
        }

        Ok(())
    }

    /// Check if creating a new file or directory is allowed under the quota.
    ///
    /// This performs a read-only check and does not modify the tracker state.
    /// Call [`record_create`](Self::record_create) after a successful creation.
    pub fn check_create(&self) -> Result<()> {
        let quota = &self.inner.quota;

        if quota.max_file_count > 0 {
            let current = self.inner.file_count.load(Ordering::Relaxed);
            if current >= quota.max_file_count {
                return Err(Error::Execution(format!(
                    "VFS quota: file count {} has reached limit {}",
                    current, quota.max_file_count
                )));
            }
        }

        Ok(())
    }

    /// Check if a path depth is within the allowed limit.
    ///
    /// The depth is the number of components in the path (e.g., `/a/b/c` has
    /// depth 3).
    pub fn check_path_depth(&self, depth: u32) -> Result<()> {
        let quota = &self.inner.quota;

        if quota.max_path_depth > 0 && depth > quota.max_path_depth {
            return Err(Error::Execution(format!(
                "VFS quota: path depth {} exceeds maximum allowed depth {}",
                depth, quota.max_path_depth
            )));
        }

        Ok(())
    }

    /// Record that bytes were written (increases tracked usage).
    pub fn record_write(&self, size: u64) {
        self.inner.current_bytes.fetch_add(size, Ordering::Relaxed);
    }

    /// Record that a file or directory was created (increases file count).
    pub fn record_create(&self) {
        self.inner.file_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that a file was deleted (decreases tracked usage).
    ///
    /// The `size` parameter should be the size of the deleted file. Both
    /// the byte count and file count are decremented.
    pub fn record_delete(&self, size: u64) {
        self.inner.current_bytes.fetch_sub(
            size.min(self.inner.current_bytes.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
        let current_count = self.inner.file_count.load(Ordering::Relaxed);
        if current_count > 0 {
            self.inner.file_count.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Get current quota usage statistics.
    pub fn usage(&self) -> QuotaUsage {
        let current_bytes = self.inner.current_bytes.load(Ordering::Relaxed);
        let file_count = self.inner.file_count.load(Ordering::Relaxed);
        let quota = &self.inner.quota;

        let bytes_used_percent = if quota.max_total_bytes > 0 {
            (current_bytes as f64 / quota.max_total_bytes as f64) * 100.0
        } else {
            0.0
        };

        let files_used_percent = if quota.max_file_count > 0 {
            (file_count as f64 / quota.max_file_count as f64) * 100.0
        } else {
            0.0
        };

        QuotaUsage {
            current_bytes,
            file_count,
            max_total_bytes: quota.max_total_bytes,
            max_file_count: quota.max_file_count,
            bytes_used_percent,
            files_used_percent,
        }
    }

    /// Get a reference to the underlying quota configuration.
    pub fn quota(&self) -> &VfsQuota {
        &self.inner.quota
    }

    /// Reset all usage counters to zero.
    pub fn reset(&self) {
        self.inner.current_bytes.store(0, Ordering::Relaxed);
        self.inner.file_count.store(0, Ordering::Relaxed);
    }
}

impl Clone for VfsQuotaTracker {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

/// Current quota usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaUsage {
    /// Current total bytes used.
    pub current_bytes: u64,
    /// Current number of files and directories.
    pub file_count: u64,
    /// Maximum total bytes allowed (from quota).
    pub max_total_bytes: u64,
    /// Maximum file count allowed (from quota).
    pub max_file_count: u64,
    /// Percentage of byte quota used (0.0 - 100.0).
    pub bytes_used_percent: f64,
    /// Percentage of file count quota used (0.0 - 100.0).
    pub files_used_percent: f64,
}

impl QuotaUsage {
    /// Returns true if the byte usage is above the given threshold percentage.
    pub fn bytes_above_threshold(&self, threshold_percent: f64) -> bool {
        self.bytes_used_percent > threshold_percent
    }

    /// Returns true if the file count usage is above the given threshold percentage.
    pub fn files_above_threshold(&self, threshold_percent: f64) -> bool {
        self.files_used_percent > threshold_percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_quota() -> VfsQuota {
        VfsQuota { max_total_bytes: 1024, max_file_count: 5, max_file_size: 512, max_path_depth: 4 }
    }

    #[test]
    fn test_check_write_within_limits() {
        let tracker = VfsQuotaTracker::new(test_quota());
        assert!(tracker.check_write(256).is_ok());
    }

    #[test]
    fn test_check_write_exceeds_file_size() {
        let tracker = VfsQuotaTracker::new(test_quota());
        // Single file exceeds max_file_size of 512.
        let result = tracker.check_write(600);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_write_exceeds_total_bytes() {
        let tracker = VfsQuotaTracker::new(test_quota());

        // Record some usage first.
        tracker.record_write(800);

        // Now trying to write 300 more would exceed max_total_bytes of 1024.
        let result = tracker.check_write(300);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_create_within_limits() {
        let tracker = VfsQuotaTracker::new(test_quota());
        assert!(tracker.check_create().is_ok());
    }

    #[test]
    fn test_check_create_exceeds_limit() {
        let tracker = VfsQuotaTracker::new(test_quota());

        // Create up to the limit.
        for _ in 0..5 {
            tracker.record_create();
        }

        // Next create should fail.
        let result = tracker.check_create();
        assert!(result.is_err());
    }

    #[test]
    fn test_check_path_depth_within_limits() {
        let tracker = VfsQuotaTracker::new(test_quota());
        assert!(tracker.check_path_depth(3).is_ok());
    }

    #[test]
    fn test_check_path_depth_exceeds_limit() {
        let tracker = VfsQuotaTracker::new(test_quota());
        // max_path_depth is 4.
        let result = tracker.check_path_depth(5);
        assert!(result.is_err());
    }

    #[test]
    fn test_record_write() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(100);
        tracker.record_write(200);

        let usage = tracker.usage();
        assert_eq!(usage.current_bytes, 300);
    }

    #[test]
    fn test_record_create() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_create();
        tracker.record_create();

        let usage = tracker.usage();
        assert_eq!(usage.file_count, 2);
    }

    #[test]
    fn test_record_delete() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(500);
        tracker.record_create();
        tracker.record_create();

        tracker.record_delete(200);

        let usage = tracker.usage();
        assert_eq!(usage.current_bytes, 300);
        assert_eq!(usage.file_count, 1);
    }

    #[test]
    fn test_record_delete_does_not_underflow() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(100);
        tracker.record_create();

        // Try to delete more than exists.
        tracker.record_delete(200);

        let usage = tracker.usage();
        assert_eq!(usage.current_bytes, 0);
        assert_eq!(usage.file_count, 0);
    }

    #[test]
    fn test_usage_percentages() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(512); // 50% of 1024
        tracker.record_create();
        tracker.record_create(); // 40% of 5

        let usage = tracker.usage();
        assert!((usage.bytes_used_percent - 50.0).abs() < f64::EPSILON);
        assert!((usage.files_used_percent - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_usage_thresholds() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(900); // ~87.9%

        let usage = tracker.usage();
        assert!(usage.bytes_above_threshold(80.0));
        assert!(!usage.bytes_above_threshold(90.0));
    }

    #[test]
    fn test_quota_presets() {
        let restrictive = VfsQuota::restrictive();
        assert_eq!(restrictive.max_total_bytes, 10 * 1024 * 1024);
        assert_eq!(restrictive.max_file_count, 100);

        let permissive = VfsQuota::permissive();
        assert!(permissive.max_total_bytes > restrictive.max_total_bytes);
        assert!(permissive.max_file_count > restrictive.max_file_count);

        let unlimited = VfsQuota::unlimited();
        assert_eq!(unlimited.max_total_bytes, 0);
        assert_eq!(unlimited.max_file_count, 0);
        assert_eq!(unlimited.max_file_size, 0);
        assert_eq!(unlimited.max_path_depth, 0);
    }

    #[test]
    fn test_unlimited_quota_allows_everything() {
        let tracker = VfsQuotaTracker::new(VfsQuota::unlimited());

        // All checks should pass with unlimited quota.
        assert!(tracker.check_write(u64::MAX / 2).is_ok());
        assert!(tracker.check_create().is_ok());
        assert!(tracker.check_path_depth(u32::MAX).is_ok());
    }

    #[test]
    fn test_reset() {
        let tracker = VfsQuotaTracker::new(test_quota());

        tracker.record_write(500);
        tracker.record_create();
        tracker.record_create();

        tracker.reset();

        let usage = tracker.usage();
        assert_eq!(usage.current_bytes, 0);
        assert_eq!(usage.file_count, 0);
    }

    #[test]
    fn test_clone_shares_state() {
        let tracker = VfsQuotaTracker::new(test_quota());
        let tracker2 = tracker.clone();

        tracker.record_write(100);
        tracker.record_create();

        // The clone should see the same state.
        let usage = tracker2.usage();
        assert_eq!(usage.current_bytes, 100);
        assert_eq!(usage.file_count, 1);
    }

    #[test]
    fn test_debug_format() {
        let tracker = VfsQuotaTracker::new(test_quota());
        let debug_str = format!("{:?}", tracker);
        assert!(debug_str.contains("VfsQuotaTracker"));
    }

    #[test]
    fn test_quota_usage_display_values() {
        let tracker = VfsQuotaTracker::new(test_quota());

        let usage = tracker.usage();
        assert_eq!(usage.max_total_bytes, 1024);
        assert_eq!(usage.max_file_count, 5);
        assert_eq!(usage.current_bytes, 0);
        assert_eq!(usage.file_count, 0);
        assert!((usage.bytes_used_percent - 0.0).abs() < f64::EPSILON);
        assert!((usage.files_used_percent - 0.0).abs() < f64::EPSILON);
    }
}
