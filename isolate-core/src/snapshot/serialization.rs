//! Snapshot serialization and persistence.
//!
//! This module provides efficient serialization of snapshots for
//! persistence, transfer, and caching.

use super::{GlobalValue, MemoryPage, Snapshot, SnapshotId, SnapshotMetadata};
use crate::config::ModuleHash;
use crate::error::{Error, Result};
use crate::sandbox::SandboxId;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Snapshot format version for backwards compatibility.
const FORMAT_VERSION: u32 = 1;

/// Magic bytes for snapshot files.
const MAGIC: &[u8; 4] = b"ISOL";

/// Header for serialized snapshots.
#[derive(Debug, Serialize, Deserialize)]
struct SnapshotHeader {
    /// Magic bytes.
    magic: [u8; 4],
    /// Format version.
    version: u32,
    /// Snapshot ID.
    snapshot_id: SnapshotId,
    /// Sandbox ID.
    sandbox_id: SandboxId,
    /// Module hash.
    module_hash: ModuleHash,
    /// Creation timestamp.
    created_at: DateTime<Utc>,
    /// Total memory size.
    memory_size: usize,
    /// Page size.
    page_size: usize,
    /// Number of memory pages.
    num_pages: usize,
    /// Number of globals.
    num_globals: usize,
    /// Number of tables.
    num_tables: usize,
    /// Fuel remaining.
    fuel_remaining: Option<u64>,
    /// Memory checksum.
    memory_checksum: String,
    /// Parent snapshot ID.
    parent_id: Option<SnapshotId>,
    /// Header checksum.
    header_checksum: String,
}

/// Serialized page entry.
#[derive(Debug, Serialize, Deserialize)]
enum SerializedPage {
    /// Zero page.
    Zero,
    /// Data page with compressed data.
    Data {
        /// Page index.
        index: usize,
        /// Original size.
        original_size: usize,
        /// Compressed data.
        data: Vec<u8>,
    },
    /// Reference to parent snapshot.
    Reference {
        /// Page index.
        index: usize,
        /// Parent snapshot ID.
        parent_id: SnapshotId,
        /// Parent page index.
        parent_index: usize,
    },
}

/// Snapshot serializer with compression support.
pub struct SnapshotSerializer {
    /// Enable compression.
    compression_enabled: bool,
    /// Compression level (0-9).
    compression_level: u32,
}

impl SnapshotSerializer {
    /// Create a new serializer with default settings.
    pub fn new() -> Self {
        Self {
            compression_enabled: true,
            compression_level: 6,
        }
    }

    /// Enable or disable compression.
    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.compression_enabled = enabled;
        self
    }

    /// Set compression level.
    pub fn with_compression_level(mut self, level: u32) -> Self {
        self.compression_level = level.min(9);
        self
    }

    /// Serialize a snapshot to bytes.
    pub fn serialize(&self, snapshot: &Snapshot) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();

        // Create header
        let header = SnapshotHeader {
            magic: *MAGIC,
            version: FORMAT_VERSION,
            snapshot_id: snapshot.id,
            sandbox_id: snapshot.sandbox_id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            memory_size: snapshot.memory_size,
            page_size: snapshot.page_size,
            num_pages: snapshot.memory_pages.len(),
            num_globals: snapshot.globals.len(),
            num_tables: snapshot.tables.len(),
            fuel_remaining: snapshot.fuel_remaining,
            memory_checksum: snapshot.memory_checksum.clone(),
            parent_id: snapshot.parent_id,
            header_checksum: String::new(), // Will be computed
        };

        // Serialize header
        let header_bytes = serde_json::to_vec(&header)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize header: {}", e)))?;

        // Write header length and data
        buffer.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&header_bytes);

        // Serialize pages
        let pages: Vec<SerializedPage> = snapshot
            .memory_pages
            .iter()
            .map(|(index, page)| self.serialize_page(*index, page))
            .collect();

        let pages_bytes = serde_json::to_vec(&pages)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize pages: {}", e)))?;

        buffer.extend_from_slice(&(pages_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&pages_bytes);

        // Serialize globals
        let globals_bytes = serde_json::to_vec(&snapshot.globals)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize globals: {}", e)))?;

        buffer.extend_from_slice(&(globals_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&globals_bytes);

        // Serialize tables
        let tables_bytes = serde_json::to_vec(&snapshot.tables)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize tables: {}", e)))?;

        buffer.extend_from_slice(&(tables_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&tables_bytes);

        // Serialize metadata
        let metadata_bytes = serde_json::to_vec(&snapshot.metadata)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize metadata: {}", e)))?;

        buffer.extend_from_slice(&(metadata_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&metadata_bytes);

        // Add final checksum
        let mut hasher = Sha256::new();
        hasher.update(&buffer);
        let checksum = hex::encode(hasher.finalize());
        buffer.extend_from_slice(checksum.as_bytes());

        Ok(buffer)
    }

    /// Deserialize a snapshot from bytes.
    pub fn deserialize(&self, data: &[u8]) -> Result<Snapshot> {
        if data.len() < 4 {
            return Err(Error::Snapshot("Data too short".into()));
        }

        let mut offset = 0;

        // Read header
        let header_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        offset += 4;

        if offset + header_len > data.len() {
            return Err(Error::Snapshot("Invalid header length".into()));
        }

        let header: SnapshotHeader =
            serde_json::from_slice(&data[offset..offset + header_len])
                .map_err(|e| Error::Snapshot(format!("Failed to parse header: {}", e)))?;

        // Verify magic
        if header.magic != *MAGIC {
            return Err(Error::Snapshot("Invalid snapshot magic".into()));
        }

        // Verify version
        if header.version > FORMAT_VERSION {
            return Err(Error::Snapshot(format!(
                "Unsupported snapshot version: {}",
                header.version
            )));
        }

        offset += header_len;

        // Read pages
        let pages_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        let pages: Vec<SerializedPage> =
            serde_json::from_slice(&data[offset..offset + pages_len])
                .map_err(|e| Error::Snapshot(format!("Failed to parse pages: {}", e)))?;

        offset += pages_len;

        // Read globals
        let globals_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        let globals: Vec<GlobalValue> =
            serde_json::from_slice(&data[offset..offset + globals_len])
                .map_err(|e| Error::Snapshot(format!("Failed to parse globals: {}", e)))?;

        offset += globals_len;

        // Read tables
        let tables_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        let tables: HashMap<String, Vec<Option<u32>>> =
            serde_json::from_slice(&data[offset..offset + tables_len])
                .map_err(|e| Error::Snapshot(format!("Failed to parse tables: {}", e)))?;

        offset += tables_len;

        // Read metadata
        let metadata_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        let metadata: SnapshotMetadata =
            serde_json::from_slice(&data[offset..offset + metadata_len])
                .map_err(|e| Error::Snapshot(format!("Failed to parse metadata: {}", e)))?;

        // Convert pages
        let memory_pages: HashMap<usize, MemoryPage> = pages
            .into_iter()
            .map(|p| self.deserialize_page(p))
            .collect();

        Ok(Snapshot {
            id: header.snapshot_id,
            sandbox_id: header.sandbox_id,
            module_hash: header.module_hash,
            created_at: header.created_at,
            memory_pages,
            memory_size: header.memory_size,
            page_size: header.page_size,
            globals,
            tables,
            fuel_remaining: header.fuel_remaining,
            memory_checksum: header.memory_checksum,
            parent_id: header.parent_id,
            metadata,
        })
    }

    /// Serialize to a file.
    pub fn serialize_to_file(&self, snapshot: &Snapshot, path: &Path) -> Result<()> {
        let data = self.serialize(snapshot)?;
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&data)?;
        Ok(())
    }

    /// Deserialize from a file.
    pub fn deserialize_from_file(&self, path: &Path) -> Result<Snapshot> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        self.deserialize(&data)
    }

    fn serialize_page(&self, index: usize, page: &MemoryPage) -> SerializedPage {
        match page {
            MemoryPage::Zero => SerializedPage::Zero,
            MemoryPage::Data(data) => {
                let compressed = if self.compression_enabled {
                    self.compress(data)
                } else {
                    data.clone()
                };
                SerializedPage::Data {
                    index,
                    original_size: data.len(),
                    data: compressed,
                }
            }
            MemoryPage::Reference {
                parent_id,
                page_index,
            } => SerializedPage::Reference {
                index,
                parent_id: *parent_id,
                parent_index: *page_index,
            },
        }
    }

    fn deserialize_page(&self, page: SerializedPage) -> (usize, MemoryPage) {
        match page {
            SerializedPage::Zero => (0, MemoryPage::Zero),
            SerializedPage::Data {
                index,
                original_size,
                data,
            } => {
                let decompressed = if self.compression_enabled && data.len() != original_size {
                    self.decompress(&data, original_size)
                } else {
                    data
                };
                (index, MemoryPage::Data(decompressed))
            }
            SerializedPage::Reference {
                index,
                parent_id,
                parent_index,
            } => (
                index,
                MemoryPage::Reference {
                    parent_id,
                    page_index: parent_index,
                },
            ),
        }
    }

    fn compress(&self, data: &[u8]) -> Vec<u8> {
        // Simple RLE compression for sparse data
        // In production, use zstd or lz4
        let mut compressed = Vec::new();
        let mut i = 0;

        while i < data.len() {
            let byte = data[i];
            let mut count = 1;

            while i + count < data.len() && data[i + count] == byte && count < 255 {
                count += 1;
            }

            if count >= 4 || byte == 0 {
                // RLE encode
                compressed.push(0xFF); // Escape byte
                compressed.push(byte);
                compressed.push(count as u8);
            } else {
                for _ in 0..count {
                    compressed.push(byte);
                }
            }

            i += count;
        }

        // Only use compressed if smaller
        if compressed.len() < data.len() {
            compressed
        } else {
            data.to_vec()
        }
    }

    fn decompress(&self, data: &[u8], original_size: usize) -> Vec<u8> {
        let mut decompressed = Vec::with_capacity(original_size);
        let mut i = 0;

        while i < data.len() && decompressed.len() < original_size {
            if data[i] == 0xFF && i + 2 < data.len() {
                // RLE encoded
                let byte = data[i + 1];
                let count = data[i + 2] as usize;
                for _ in 0..count {
                    if decompressed.len() < original_size {
                        decompressed.push(byte);
                    }
                }
                i += 3;
            } else {
                decompressed.push(data[i]);
                i += 1;
            }
        }

        decompressed
    }
}

impl Default for SnapshotSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming snapshot writer for large snapshots.
pub struct SnapshotWriter<W: Write> {
    writer: BufWriter<W>,
    pages_written: usize,
}

impl<W: Write> SnapshotWriter<W> {
    /// Create a new streaming writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
            pages_written: 0,
        }
    }

    /// Write the snapshot header.
    pub fn write_header(&mut self, snapshot: &Snapshot) -> Result<()> {
        let header = SnapshotHeader {
            magic: *MAGIC,
            version: FORMAT_VERSION,
            snapshot_id: snapshot.id,
            sandbox_id: snapshot.sandbox_id,
            module_hash: snapshot.module_hash.clone(),
            created_at: snapshot.created_at,
            memory_size: snapshot.memory_size,
            page_size: snapshot.page_size,
            num_pages: snapshot.memory_pages.len(),
            num_globals: snapshot.globals.len(),
            num_tables: snapshot.tables.len(),
            fuel_remaining: snapshot.fuel_remaining,
            memory_checksum: snapshot.memory_checksum.clone(),
            parent_id: snapshot.parent_id,
            header_checksum: String::new(),
        };

        let header_bytes = serde_json::to_vec(&header)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize header: {}", e)))?;

        self.writer
            .write_all(&(header_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(&header_bytes)?;

        Ok(())
    }

    /// Write a memory page.
    pub fn write_page(&mut self, index: usize, page: &MemoryPage) -> Result<()> {
        let serialized = match page {
            MemoryPage::Zero => SerializedPage::Zero,
            MemoryPage::Data(data) => SerializedPage::Data {
                index,
                original_size: data.len(),
                data: data.clone(),
            },
            MemoryPage::Reference {
                parent_id,
                page_index,
            } => SerializedPage::Reference {
                index,
                parent_id: *parent_id,
                parent_index: *page_index,
            },
        };

        let page_bytes = serde_json::to_vec(&serialized)
            .map_err(|e| Error::Snapshot(format!("Failed to serialize page: {}", e)))?;

        self.writer
            .write_all(&(page_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(&page_bytes)?;
        self.pages_written += 1;

        Ok(())
    }

    /// Finish writing and return the underlying writer.
    pub fn finish(mut self) -> Result<W> {
        self.writer.flush()?;
        Ok(self.writer.into_inner().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Snapshot;

    #[test]
    fn test_serializer_roundtrip() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        let mut memory = vec![0u8; 65536];
        memory[0..5].copy_from_slice(b"hello");

        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![]);

        let serializer = SnapshotSerializer::new();
        let bytes = serializer.serialize(&snapshot).unwrap();
        let restored = serializer.deserialize(&bytes).unwrap();

        assert_eq!(snapshot.id, restored.id);
        assert_eq!(snapshot.memory_size, restored.memory_size);
        assert_eq!(snapshot.memory_checksum, restored.memory_checksum);
    }

    #[test]
    fn test_compression() {
        let serializer = SnapshotSerializer::new().with_compression(true);

        // Highly compressible data
        let data = vec![0u8; 4096];
        let compressed = serializer.compress(&data);

        assert!(compressed.len() < data.len());

        let decompressed = serializer.decompress(&compressed, data.len());
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_serializer_no_compression() {
        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());

        let memory = vec![0u8; 65536];
        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![]);

        let serializer = SnapshotSerializer::new().with_compression(false);
        let bytes = serializer.serialize(&snapshot).unwrap();
        let restored = serializer.deserialize(&bytes).unwrap();

        assert_eq!(snapshot.id, restored.id);
    }

    #[test]
    fn test_file_serialization() {
        use tempfile::NamedTempFile;

        let sandbox_id = SandboxId::new();
        let module_hash = ModuleHash("test123".to_string());
        let memory = vec![0u8; 65536];
        let snapshot = Snapshot::from_memory(sandbox_id, module_hash, &memory, vec![]);

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let serializer = SnapshotSerializer::new();
        serializer.serialize_to_file(&snapshot, path).unwrap();

        let restored = serializer.deserialize_from_file(path).unwrap();
        assert_eq!(snapshot.id, restored.id);
    }
}
