//! Layered virtual filesystem implementation.
//!
//! Provides an overlay filesystem model where multiple layers are stacked
//! on top of each other. Reads search from top to bottom, while writes
//! always target the topmost writable layer.

use crate::error::{Error, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Permissions on a virtual filesystem node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsPermissions {
    /// Whether the node can be read.
    pub read: bool,
    /// Whether the node can be written to.
    pub write: bool,
    /// Whether the node can be executed.
    pub execute: bool,
}

impl VfsPermissions {
    /// Create permissions with all flags set to true.
    pub fn all() -> Self {
        Self { read: true, write: true, execute: true }
    }

    /// Create read-only permissions.
    pub fn read_only() -> Self {
        Self { read: true, write: false, execute: false }
    }

    /// Create read-write permissions.
    pub fn read_write() -> Self {
        Self { read: true, write: true, execute: false }
    }
}

impl Default for VfsPermissions {
    fn default() -> Self {
        Self::read_write()
    }
}

/// A node in the virtual filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VfsNode {
    /// A regular file.
    File {
        /// File content as raw bytes.
        content: Vec<u8>,
        /// File permissions.
        permissions: VfsPermissions,
        /// File size in bytes (derived from content length).
        size: u64,
        /// Last modification time as milliseconds since UNIX epoch.
        modified_at: u64,
    },
    /// A directory.
    Directory {
        /// Child entry names in this directory.
        children: Vec<String>,
        /// Directory permissions.
        permissions: VfsPermissions,
    },
    /// A symbolic link.
    Symlink {
        /// Target path the symlink points to.
        target: PathBuf,
    },
}

impl VfsNode {
    /// Create a new file node with the given content.
    pub fn file(content: Vec<u8>, permissions: VfsPermissions) -> Self {
        let size = content.len() as u64;
        let modified_at =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        Self::File { content, permissions, size, modified_at }
    }

    /// Create a new empty directory node.
    pub fn directory(permissions: VfsPermissions) -> Self {
        Self::Directory { children: Vec::new(), permissions }
    }

    /// Create a new symlink node.
    pub fn symlink(target: PathBuf) -> Self {
        Self::Symlink { target }
    }

    /// Returns true if this node is a file.
    pub fn is_file(&self) -> bool {
        matches!(self, VfsNode::File { .. })
    }

    /// Returns true if this node is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, VfsNode::Directory { .. })
    }

    /// Returns true if this node is a symlink.
    pub fn is_symlink(&self) -> bool {
        matches!(self, VfsNode::Symlink { .. })
    }
}

/// A single filesystem layer with a name and a set of nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsLayer {
    /// Human-readable name of this layer.
    name: String,
    /// Map from absolute paths to filesystem nodes.
    nodes: HashMap<PathBuf, VfsNode>,
    /// Whether this layer is read-only.
    read_only: bool,
}

impl VfsLayer {
    /// Create a new empty filesystem layer.
    pub fn new(name: impl Into<String>, read_only: bool) -> Self {
        Self { name: name.into(), nodes: HashMap::new(), read_only }
    }

    /// Get the name of this layer.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns true if this layer is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Get a node at the given path, if it exists in this layer.
    pub fn get(&self, path: &Path) -> Option<&VfsNode> {
        self.nodes.get(path)
    }

    /// Insert a node at the given path in this layer.
    pub fn insert(&mut self, path: PathBuf, node: VfsNode) {
        self.nodes.insert(path, node);
    }

    /// Remove a node at the given path from this layer.
    pub fn remove(&mut self, path: &Path) -> Option<VfsNode> {
        self.nodes.remove(path)
    }

    /// Check if a node exists at the given path in this layer.
    pub fn contains(&self, path: &Path) -> bool {
        self.nodes.contains_key(path)
    }

    /// Get the number of nodes in this layer.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if this layer has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// File statistics returned by [`VirtualFilesystem::stat`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsStat {
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Whether this is a regular file.
    pub is_file: bool,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Permissions on this node.
    pub permissions: VfsPermissions,
    /// Last modification time as milliseconds since UNIX epoch.
    pub modified_at: u64,
}

/// A virtual filesystem with multiple overlay layers.
///
/// Layers are stacked in order, with later layers taking priority over
/// earlier ones when reading. Writes always go to the topmost writable
/// layer. This allows a read-only base layer (e.g., system files) to be
/// overlaid with a writable scratch layer for sandbox modifications.
///
/// # Example
///
/// ```rust
/// use isolate_core::vfs::{VirtualFilesystem, VfsPermissions};
/// use std::path::PathBuf;
///
/// let mut vfs = VirtualFilesystem::new();
/// vfs.add_layer("base", true);  // read-only base
/// vfs.add_layer("scratch", false);  // writable scratch
///
/// // Mount content into the base layer
/// vfs.mount("base", &PathBuf::from("/etc/config"), b"key=value".to_vec()).unwrap();
///
/// // Read searches layers top-to-bottom
/// let content = vfs.read(&PathBuf::from("/etc/config")).unwrap();
/// assert_eq!(content, b"key=value");
/// ```
pub struct VirtualFilesystem {
    /// Ordered list of layers (last = highest priority).
    layers: Arc<RwLock<Vec<VfsLayer>>>,
}

impl VirtualFilesystem {
    /// Create a new empty virtual filesystem with no layers.
    pub fn new() -> Self {
        Self { layers: Arc::new(RwLock::new(Vec::new())) }
    }

    /// Add a new layer to the top of the layer stack.
    ///
    /// Layers added later have higher priority when reading. The `read_only`
    /// flag prevents writes to this layer.
    pub fn add_layer(&mut self, name: impl Into<String>, read_only: bool) {
        let mut layers = self.layers.write();
        layers.push(VfsLayer::new(name, read_only));
    }

    /// Mount content at a path within a specific named layer.
    ///
    /// This creates a file node at the given path, automatically creating
    /// parent directories as needed. The layer must exist and, if the layer
    /// is read-only, this is the only way to populate it (direct mount).
    pub fn mount(&self, layer_name: &str, path: &Path, content: Vec<u8>) -> Result<()> {
        let normalized = normalize_path(path);
        let mut layers = self.layers.write();

        let layer = layers
            .iter_mut()
            .find(|l| l.name() == layer_name)
            .ok_or_else(|| Error::InvalidConfig(format!("VFS layer not found: {}", layer_name)))?;

        // Ensure parent directories exist within this layer.
        ensure_parent_dirs(layer, &normalized);

        let node = VfsNode::file(content, VfsPermissions::default());
        layer.insert(normalized, node);

        Ok(())
    }

    /// Read the contents of a file at the given path.
    ///
    /// Searches layers from top (highest priority) to bottom. Returns the
    /// content from the first layer that contains the path.
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let normalized = normalize_path(path);

        // We hold the read lock in a limited scope to allow symlink recursion.
        enum ReadResult {
            Content(Vec<u8>),
            IsDirectory(PathBuf),
            FollowSymlink(PathBuf),
            AccessDenied(PathBuf),
            NotFound(PathBuf),
        }

        let result = {
            let layers = self.layers.read();

            let mut outcome = ReadResult::NotFound(normalized.clone());

            for layer in layers.iter().rev() {
                if let Some(node) = layer.get(&normalized) {
                    outcome = match node {
                        VfsNode::File { content, permissions, .. } => {
                            if !permissions.read {
                                ReadResult::AccessDenied(normalized.clone())
                            } else {
                                ReadResult::Content(content.clone())
                            }
                        }
                        VfsNode::Directory { .. } => ReadResult::IsDirectory(normalized.clone()),
                        VfsNode::Symlink { target } => ReadResult::FollowSymlink(target.clone()),
                    };
                    break;
                }
            }

            outcome
        }; // Lock is dropped here.

        match result {
            ReadResult::Content(data) => Ok(data),
            ReadResult::IsDirectory(p) => {
                Err(Error::Execution(format!("VFS: '{}' is a directory, not a file", p.display())))
            }
            ReadResult::FollowSymlink(target) => self.read(&target),
            ReadResult::AccessDenied(p) => Err(Error::FilesystemAccessDenied { path: p }),
            ReadResult::NotFound(p) => {
                Err(Error::Execution(format!("VFS: file not found: {}", p.display())))
            }
        }
    }

    /// Write content to a file at the given path.
    ///
    /// The write targets the topmost writable layer. If the file does not
    /// exist, it is created. Parent directories are created automatically.
    pub fn write(&self, path: &Path, content: Vec<u8>) -> Result<()> {
        let normalized = normalize_path(path);
        let mut layers = self.layers.write();

        // Find topmost writable layer (last non-read-only).
        let writable_layer = layers
            .iter_mut()
            .rev()
            .find(|l| !l.is_read_only())
            .ok_or_else(|| Error::Execution("VFS: no writable layer available".to_string()))?;

        ensure_parent_dirs(writable_layer, &normalized);

        let node = VfsNode::file(content, VfsPermissions::default());
        writable_layer.insert(normalized, node);

        Ok(())
    }

    /// Create a directory at the given path.
    ///
    /// The directory is created in the topmost writable layer. Parent
    /// directories are created automatically.
    pub fn mkdir(&self, path: &Path) -> Result<()> {
        let normalized = normalize_path(path);
        let mut layers = self.layers.write();

        // Check if directory already exists in any layer.
        for layer in layers.iter().rev() {
            if let Some(node) = layer.get(&normalized) {
                if node.is_dir() {
                    return Ok(()); // Already exists.
                }
                return Err(Error::Execution(format!(
                    "VFS: path already exists and is not a directory: {}",
                    normalized.display()
                )));
            }
        }

        let writable_layer = layers
            .iter_mut()
            .rev()
            .find(|l| !l.is_read_only())
            .ok_or_else(|| Error::Execution("VFS: no writable layer available".to_string()))?;

        ensure_parent_dirs(writable_layer, &normalized);

        let node = VfsNode::directory(VfsPermissions::default());
        writable_layer.insert(normalized, node);

        Ok(())
    }

    /// Remove a file or directory at the given path.
    ///
    /// The entry is removed from the topmost writable layer. If the entry
    /// only exists in a read-only layer, the removal fails.
    pub fn remove(&self, path: &Path) -> Result<()> {
        let normalized = normalize_path(path);
        let mut layers = self.layers.write();

        // First, verify the path exists somewhere.
        let exists = layers.iter().any(|l| l.contains(&normalized));
        if !exists {
            return Err(Error::Execution(format!("VFS: path not found: {}", normalized.display())));
        }

        // Try to remove from a writable layer.
        let mut removed = false;
        for layer in layers.iter_mut().rev() {
            if !layer.is_read_only() && layer.contains(&normalized) {
                layer.remove(&normalized);
                removed = true;
                break;
            }
        }

        if !removed {
            // The entry exists only in read-only layers.
            return Err(Error::FilesystemAccessDenied { path: normalized });
        }

        Ok(())
    }

    /// Check if a path exists in any layer.
    pub fn exists(&self, path: &Path) -> bool {
        let normalized = normalize_path(path);
        let layers = self.layers.read();
        layers.iter().any(|l| l.contains(&normalized))
    }

    /// List the contents of a directory.
    ///
    /// Aggregates children from all layers, with higher-priority layers
    /// taking precedence. Returns a sorted, deduplicated list of entry names.
    pub fn list(&self, path: &Path) -> Result<Vec<String>> {
        let normalized = normalize_path(path);
        let layers = self.layers.read();

        let mut found_dir = false;
        let mut children: Vec<String> = Vec::new();

        // Collect children from all layers.
        for layer in layers.iter().rev() {
            if let Some(node) = layer.get(&normalized) {
                match node {
                    VfsNode::Directory { children: dir_children, .. } => {
                        found_dir = true;
                        for child in dir_children {
                            if !children.contains(child) {
                                children.push(child.clone());
                            }
                        }
                    }
                    _ => {
                        return Err(Error::Execution(format!(
                            "VFS: '{}' is not a directory",
                            normalized.display()
                        )));
                    }
                }
            }
        }

        // Also look for nodes that are direct children of this path.
        for layer in layers.iter() {
            for node_path in layer.nodes.keys() {
                if let Some(parent) = node_path.parent() {
                    if parent == normalized {
                        if let Some(name) = node_path.file_name() {
                            let name_str = name.to_string_lossy().to_string();
                            if !children.contains(&name_str) {
                                children.push(name_str);
                                found_dir = true;
                            }
                        }
                    }
                }
            }
        }

        if !found_dir {
            return Err(Error::Execution(format!(
                "VFS: directory not found: {}",
                normalized.display()
            )));
        }

        children.sort();
        children.dedup();
        Ok(children)
    }

    /// Get file statistics for a path.
    ///
    /// Returns stats from the highest-priority layer that contains the path.
    pub fn stat(&self, path: &Path) -> Result<VfsStat> {
        let normalized = normalize_path(path);

        enum StatResult {
            Found(VfsStat),
            FollowSymlink(PathBuf),
            NotFound(PathBuf),
        }

        let result = {
            let layers = self.layers.read();
            let mut outcome = StatResult::NotFound(normalized.clone());

            for layer in layers.iter().rev() {
                if let Some(node) = layer.get(&normalized) {
                    outcome = match node {
                        VfsNode::File { size, permissions, modified_at, .. } => {
                            StatResult::Found(VfsStat {
                                size: *size,
                                is_file: true,
                                is_dir: false,
                                permissions: permissions.clone(),
                                modified_at: *modified_at,
                            })
                        }
                        VfsNode::Directory { permissions, .. } => StatResult::Found(VfsStat {
                            size: 0,
                            is_file: false,
                            is_dir: true,
                            permissions: permissions.clone(),
                            modified_at: 0,
                        }),
                        VfsNode::Symlink { target } => StatResult::FollowSymlink(target.clone()),
                    };
                    break;
                }
            }

            outcome
        }; // Lock is dropped here.

        match result {
            StatResult::Found(stat) => Ok(stat),
            StatResult::FollowSymlink(target) => self.stat(&target),
            StatResult::NotFound(p) => {
                Err(Error::Execution(format!("VFS: path not found: {}", p.display())))
            }
        }
    }
}

impl Default for VirtualFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a path by resolving `.` and `..` components, stripping trailing
/// slashes, and ensuring the path is absolute (prefixed with `/`).
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    let mut has_root = false;

    for component in path.components() {
        match component {
            Component::RootDir => {
                has_root = true;
            }
            Component::CurDir => {
                // Skip `.`
            }
            Component::ParentDir => {
                // Pop the last component if possible.
                parts.pop();
            }
            Component::Normal(name) => {
                parts.push(name);
            }
            Component::Prefix(_) => {
                // Not relevant for Unix paths but handle gracefully.
            }
        }
    }

    // Always produce an absolute path. Even relative inputs are treated
    // as absolute within the virtual filesystem.
    let _ = has_root;
    let mut normalized = PathBuf::from("/");

    for part in parts {
        normalized.push(part);
    }

    normalized
}

/// Ensure all parent directories of `path` exist in the given layer.
fn ensure_parent_dirs(layer: &mut VfsLayer, path: &Path) {
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut current = path.parent();
    while let Some(p) = current {
        if p == Path::new("/") || p == Path::new("") {
            break;
        }
        ancestors.push(p.to_path_buf());
        current = p.parent();
    }

    // Insert ancestors from the root down.
    for ancestor in ancestors.into_iter().rev() {
        if !layer.contains(&ancestor) {
            // Add the directory node.
            layer.insert(ancestor.clone(), VfsNode::directory(VfsPermissions::default()));
        }
        // Also register this as a child in its parent directory.
        if let Some(parent) = ancestor.parent() {
            if let Some(name) = ancestor.file_name() {
                let name_str = name.to_string_lossy().to_string();
                if let Some(VfsNode::Directory { children, .. }) = layer.nodes.get_mut(parent) {
                    if !children.contains(&name_str) {
                        children.push(name_str);
                    }
                }
            }
        }
    }

    // Register the node itself as a child in its parent directory.
    if let Some(parent) = path.parent() {
        if let Some(name) = path.file_name() {
            let name_str = name.to_string_lossy().to_string();
            if let Some(VfsNode::Directory { children, .. }) = layer.nodes.get_mut(parent) {
                if !children.contains(&name_str) {
                    children.push(name_str);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_vfs() -> VirtualFilesystem {
        let mut vfs = VirtualFilesystem::new();
        vfs.add_layer("base", true);
        vfs.add_layer("scratch", false);
        vfs
    }

    #[test]
    fn test_new_vfs_is_empty() {
        let vfs = VirtualFilesystem::new();
        assert!(!vfs.exists(Path::new("/anything")));
    }

    #[test]
    fn test_mount_and_read() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/etc/config.txt");
        let content = b"hello world".to_vec();

        vfs.mount("base", &path, content.clone()).unwrap();
        let result = vfs.read(&path).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_mount_to_nonexistent_layer_fails() {
        let vfs = create_test_vfs();
        let result = vfs.mount("nonexistent", &PathBuf::from("/test"), vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_nonexistent_file_fails() {
        let vfs = create_test_vfs();
        let result = vfs.read(&PathBuf::from("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_write_to_writable_layer() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/tmp/data.txt");
        let content = b"written data".to_vec();

        vfs.write(&path, content.clone()).unwrap();
        let result = vfs.read(&path).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_write_no_writable_layer_fails() {
        let mut vfs = VirtualFilesystem::new();
        vfs.add_layer("readonly", true);

        let result = vfs.write(&PathBuf::from("/test"), vec![1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_overlay_priority() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/etc/config");

        // Mount in base layer.
        vfs.mount("base", &path, b"base content".to_vec()).unwrap();

        // Write to scratch layer (higher priority).
        vfs.write(&path, b"scratch content".to_vec()).unwrap();

        // Read should return scratch content.
        let result = vfs.read(&path).unwrap();
        assert_eq!(result, b"scratch content");
    }

    #[test]
    fn test_exists() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/data/file.txt");

        assert!(!vfs.exists(&path));

        vfs.write(&path, b"content".to_vec()).unwrap();
        assert!(vfs.exists(&path));
    }

    #[test]
    fn test_mkdir() {
        let vfs = create_test_vfs();
        let dir_path = PathBuf::from("/var/log");

        vfs.mkdir(&dir_path).unwrap();
        assert!(vfs.exists(&dir_path));

        let stat = vfs.stat(&dir_path).unwrap();
        assert!(stat.is_dir);
        assert!(!stat.is_file);
    }

    #[test]
    fn test_mkdir_idempotent() {
        let vfs = create_test_vfs();
        let dir_path = PathBuf::from("/var/log");

        vfs.mkdir(&dir_path).unwrap();
        // Creating again should succeed (already exists).
        vfs.mkdir(&dir_path).unwrap();
    }

    #[test]
    fn test_remove_file() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/tmp/remove_me.txt");

        vfs.write(&path, b"to be removed".to_vec()).unwrap();
        assert!(vfs.exists(&path));

        vfs.remove(&path).unwrap();
        assert!(!vfs.exists(&path));
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let vfs = create_test_vfs();
        let result = vfs.remove(&PathBuf::from("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_readonly_fails() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/etc/readonly_file");

        // Mount in the read-only base layer.
        vfs.mount("base", &path, b"data".to_vec()).unwrap();

        // Removing should fail since it's only in a read-only layer.
        let result = vfs.remove(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_stat_file() {
        let vfs = create_test_vfs();
        let path = PathBuf::from("/data/stats.txt");
        let content = b"some content here".to_vec();
        let expected_size = content.len() as u64;

        vfs.write(&path, content).unwrap();

        let stat = vfs.stat(&path).unwrap();
        assert!(stat.is_file);
        assert!(!stat.is_dir);
        assert_eq!(stat.size, expected_size);
        assert!(stat.modified_at > 0);
    }

    #[test]
    fn test_stat_nonexistent_fails() {
        let vfs = create_test_vfs();
        let result = vfs.stat(&PathBuf::from("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_list_directory() {
        let vfs = create_test_vfs();
        let dir = PathBuf::from("/mydir");

        vfs.mkdir(&dir).unwrap();
        vfs.write(&dir.join("a.txt"), b"aaa".to_vec()).unwrap();
        vfs.write(&dir.join("b.txt"), b"bbb".to_vec()).unwrap();

        let entries = vfs.list(&dir).unwrap();
        assert!(entries.contains(&"a.txt".to_string()));
        assert!(entries.contains(&"b.txt".to_string()));
    }

    #[test]
    fn test_list_nonexistent_directory_fails() {
        let vfs = create_test_vfs();
        let result = vfs.list(&PathBuf::from("/nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_read_directory_fails() {
        let vfs = create_test_vfs();
        let dir = PathBuf::from("/mydir");

        vfs.mkdir(&dir).unwrap();

        let result = vfs.read(&dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_symlink() {
        let vfs = create_test_vfs();
        let target = PathBuf::from("/data/real_file.txt");
        let link = PathBuf::from("/data/link.txt");

        vfs.write(&target, b"real content".to_vec()).unwrap();

        // Manually insert a symlink in the scratch layer.
        {
            let mut layers = vfs.layers.write();
            let scratch = layers.iter_mut().find(|l| l.name() == "scratch").unwrap();
            scratch.insert(link.clone(), VfsNode::symlink(target));
        }

        let result = vfs.read(&link).unwrap();
        assert_eq!(result, b"real content");
    }

    #[test]
    fn test_vfs_node_type_checks() {
        let file = VfsNode::file(vec![1, 2, 3], VfsPermissions::default());
        assert!(file.is_file());
        assert!(!file.is_dir());
        assert!(!file.is_symlink());

        let dir = VfsNode::directory(VfsPermissions::default());
        assert!(!dir.is_file());
        assert!(dir.is_dir());
        assert!(!dir.is_symlink());

        let link = VfsNode::symlink(PathBuf::from("/target"));
        assert!(!link.is_file());
        assert!(!link.is_dir());
        assert!(link.is_symlink());
    }

    #[test]
    fn test_vfs_permissions() {
        let all = VfsPermissions::all();
        assert!(all.read);
        assert!(all.write);
        assert!(all.execute);

        let ro = VfsPermissions::read_only();
        assert!(ro.read);
        assert!(!ro.write);
        assert!(!ro.execute);

        let rw = VfsPermissions::read_write();
        assert!(rw.read);
        assert!(rw.write);
        assert!(!rw.execute);
    }

    #[test]
    fn test_layer_operations() {
        let mut layer = VfsLayer::new("test", false);
        assert!(layer.is_empty());
        assert_eq!(layer.len(), 0);
        assert_eq!(layer.name(), "test");
        assert!(!layer.is_read_only());

        let path = PathBuf::from("/file");
        layer.insert(path.clone(), VfsNode::file(vec![], VfsPermissions::default()));
        assert!(!layer.is_empty());
        assert_eq!(layer.len(), 1);
        assert!(layer.contains(&path));

        let removed = layer.remove(&path);
        assert!(removed.is_some());
        assert!(layer.is_empty());
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path(Path::new("/a/b/c")), PathBuf::from("/a/b/c"));
        assert_eq!(normalize_path(Path::new("a/b")), PathBuf::from("/a/b"));
        assert_eq!(normalize_path(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
    }

    #[test]
    fn test_multiple_writable_layers() {
        let mut vfs = VirtualFilesystem::new();
        vfs.add_layer("lower_rw", false);
        vfs.add_layer("upper_rw", false);

        let path = PathBuf::from("/file.txt");

        // Write goes to the topmost writable layer (upper_rw).
        vfs.write(&path, b"upper".to_vec()).unwrap();

        // Verify it's in the upper layer.
        let content = vfs.read(&path).unwrap();
        assert_eq!(content, b"upper");
    }

    #[test]
    fn test_list_merges_layers() {
        let vfs = create_test_vfs();
        let dir = PathBuf::from("/merged");

        // Mount in base layer.
        vfs.mount("base", &dir.join("from_base.txt"), b"base".to_vec()).unwrap();

        // Write to scratch layer.
        vfs.write(&dir.join("from_scratch.txt"), b"scratch".to_vec()).unwrap();

        let entries = vfs.list(&dir).unwrap();
        assert!(entries.contains(&"from_base.txt".to_string()));
        assert!(entries.contains(&"from_scratch.txt".to_string()));
    }
}
