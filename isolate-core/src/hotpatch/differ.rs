//! Binary diffing for WASM modules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A chunk of difference between two modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffChunk {
    /// Offset in the original module.
    pub offset: usize,
    /// Length of the original data.
    pub original_len: usize,
    /// New data to insert.
    pub new_data: Vec<u8>,
    /// Chunk type.
    pub chunk_type: ChunkType,
}

impl DiffChunk {
    /// Create an insert chunk.
    pub fn insert(offset: usize, data: Vec<u8>) -> Self {
        Self {
            offset,
            original_len: 0,
            new_data: data,
            chunk_type: ChunkType::Insert,
        }
    }

    /// Create a delete chunk.
    pub fn delete(offset: usize, len: usize) -> Self {
        Self {
            offset,
            original_len: len,
            new_data: Vec::new(),
            chunk_type: ChunkType::Delete,
        }
    }

    /// Create a replace chunk.
    pub fn replace(offset: usize, original_len: usize, new_data: Vec<u8>) -> Self {
        Self {
            offset,
            original_len,
            new_data,
            chunk_type: ChunkType::Replace,
        }
    }

    /// Get the size change from this chunk.
    pub fn size_change(&self) -> isize {
        self.new_data.len() as isize - self.original_len as isize
    }
}

/// Type of diff chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkType {
    /// Insert new data.
    Insert,
    /// Delete existing data.
    Delete,
    /// Replace existing data.
    Replace,
    /// Copy existing data (for reference).
    Copy,
}

/// A bundle of patches to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchBundle {
    /// Bundle ID.
    pub id: String,
    /// Source module hash.
    pub source_hash: String,
    /// Target module hash.
    pub target_hash: String,
    /// Diff chunks.
    pub chunks: Vec<DiffChunk>,
    /// Expected result size.
    pub target_size: usize,
    /// Metadata.
    pub metadata: HashMap<String, String>,
    /// Compressed bundle data.
    pub compressed: Option<Vec<u8>>,
}

impl PatchBundle {
    /// Create a new patch bundle.
    pub fn new(source_hash: String, target_hash: String) -> Self {
        Self {
            id: generate_id(),
            source_hash,
            target_hash,
            chunks: Vec::new(),
            target_size: 0,
            metadata: HashMap::new(),
            compressed: None,
        }
    }

    /// Add a chunk.
    pub fn add_chunk(&mut self, chunk: DiffChunk) {
        self.chunks.push(chunk);
    }

    /// Get total patch size.
    pub fn patch_size(&self) -> usize {
        if let Some(compressed) = &self.compressed {
            compressed.len()
        } else {
            self.chunks.iter().map(|c| c.new_data.len()).sum()
        }
    }

    /// Get number of chunks.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Verify that applying this patch to source produces target.
    pub fn verify(&self, source: &[u8], target: &[u8]) -> bool {
        let result = self.apply(source);
        match result {
            Ok(patched) => patched == target,
            Err(_) => false,
        }
    }

    /// Apply the patch to source bytes.
    pub fn apply(&self, source: &[u8]) -> Result<Vec<u8>, PatchError> {
        let mut result = Vec::with_capacity(self.target_size);
        let mut source_pos = 0;

        for chunk in &self.chunks {
            // Copy unchanged data before this chunk
            if chunk.offset > source_pos {
                if chunk.offset > source.len() {
                    return Err(PatchError::InvalidOffset(chunk.offset));
                }
                result.extend_from_slice(&source[source_pos..chunk.offset]);
            }

            // Apply the chunk
            match chunk.chunk_type {
                ChunkType::Insert => {
                    result.extend_from_slice(&chunk.new_data);
                    source_pos = chunk.offset;
                }
                ChunkType::Delete => {
                    source_pos = chunk.offset + chunk.original_len;
                }
                ChunkType::Replace => {
                    result.extend_from_slice(&chunk.new_data);
                    source_pos = chunk.offset + chunk.original_len;
                }
                ChunkType::Copy => {
                    let end = chunk.offset + chunk.original_len;
                    if end > source.len() {
                        return Err(PatchError::InvalidOffset(end));
                    }
                    result.extend_from_slice(&source[chunk.offset..end]);
                    source_pos = end;
                }
            }
        }

        // Copy remaining data
        if source_pos < source.len() {
            result.extend_from_slice(&source[source_pos..]);
        }

        if result.len() != self.target_size {
            return Err(PatchError::SizeMismatch {
                expected: self.target_size,
                actual: result.len(),
            });
        }

        Ok(result)
    }
}

/// Error during patching.
#[derive(Debug, Clone)]
pub enum PatchError {
    /// Invalid offset in patch.
    InvalidOffset(usize),
    /// Size mismatch after patching.
    SizeMismatch { expected: usize, actual: usize },
    /// Hash mismatch.
    HashMismatch { expected: String, actual: String },
    /// Compression error.
    CompressionError(String),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::InvalidOffset(off) => write!(f, "Invalid offset: {}", off),
            PatchError::SizeMismatch { expected, actual } => {
                write!(f, "Size mismatch: expected {}, got {}", expected, actual)
            }
            PatchError::HashMismatch { expected, actual } => {
                write!(f, "Hash mismatch: expected {}, got {}", expected, actual)
            }
            PatchError::CompressionError(e) => write!(f, "Compression error: {}", e),
        }
    }
}

impl std::error::Error for PatchError {}

/// WASM module differ.
pub struct WasmDiffer {
    /// Minimum chunk size for diffing.
    min_chunk_size: usize,
    /// Maximum look-ahead for matching.
    max_lookahead: usize,
}

impl Default for WasmDiffer {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmDiffer {
    /// Create a new differ.
    pub fn new() -> Self {
        Self {
            min_chunk_size: 64,
            max_lookahead: 1024,
        }
    }

    /// Set minimum chunk size.
    pub fn with_min_chunk_size(mut self, size: usize) -> Self {
        self.min_chunk_size = size;
        self
    }

    /// Compute the difference between two modules.
    pub fn diff(&self, source: &[u8], target: &[u8]) -> PatchBundle {
        let source_hash = compute_hash(source);
        let target_hash = compute_hash(target);

        let mut bundle = PatchBundle::new(source_hash, target_hash);
        bundle.target_size = target.len();

        // Simple diff algorithm: find common prefixes and suffixes
        let common_prefix = self.common_prefix(source, target);
        let common_suffix = self.common_suffix(&source[common_prefix..], &target[common_prefix..]);

        let source_middle = &source[common_prefix..source.len() - common_suffix];
        let target_middle = &target[common_prefix..target.len() - common_suffix];

        if !source_middle.is_empty() || !target_middle.is_empty() {
            bundle.add_chunk(DiffChunk::replace(
                common_prefix,
                source_middle.len(),
                target_middle.to_vec(),
            ));
        }

        bundle
    }

    /// Find common prefix length.
    fn common_prefix(&self, a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }

    /// Find common suffix length.
    fn common_suffix(&self, a: &[u8], b: &[u8]) -> usize {
        a.iter()
            .rev()
            .zip(b.iter().rev())
            .take_while(|(x, y)| x == y)
            .count()
    }

    /// Create a full replacement patch (no diff).
    pub fn full_replace(&self, source: &[u8], target: &[u8]) -> PatchBundle {
        let source_hash = compute_hash(source);
        let target_hash = compute_hash(target);

        let mut bundle = PatchBundle::new(source_hash, target_hash);
        bundle.target_size = target.len();

        bundle.add_chunk(DiffChunk::replace(0, source.len(), target.to_vec()));

        bundle
    }
}

/// Compute hash of bytes.
fn compute_hash(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Generate a unique ID.
fn generate_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("patch-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_chunk_insert() {
        let chunk = DiffChunk::insert(10, vec![1, 2, 3]);
        assert_eq!(chunk.offset, 10);
        assert_eq!(chunk.original_len, 0);
        assert_eq!(chunk.size_change(), 3);
    }

    #[test]
    fn test_diff_chunk_delete() {
        let chunk = DiffChunk::delete(10, 5);
        assert_eq!(chunk.size_change(), -5);
    }

    #[test]
    fn test_diff_chunk_replace() {
        let chunk = DiffChunk::replace(10, 5, vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(chunk.size_change(), 2);
    }

    #[test]
    fn test_patch_bundle_apply() {
        let source = b"Hello World";
        let target = b"Hello Rust";

        let differ = WasmDiffer::new();
        let bundle = differ.diff(source, target);

        let result = bundle.apply(source).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn test_patch_bundle_verify() {
        let source = b"Hello World";
        let target = b"Hello Rust";

        let differ = WasmDiffer::new();
        let bundle = differ.diff(source, target);

        assert!(bundle.verify(source, target));
        assert!(!bundle.verify(source, b"Wrong"));
    }

    #[test]
    fn test_full_replace() {
        let source = b"Old content";
        let target = b"New content here";

        let differ = WasmDiffer::new();
        let bundle = differ.full_replace(source, target);

        let result = bundle.apply(source).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn test_identical_modules() {
        let source = b"Same content";
        let target = b"Same content";

        let differ = WasmDiffer::new();
        let bundle = differ.diff(source, target);

        assert_eq!(bundle.chunk_count(), 0);
    }
}
