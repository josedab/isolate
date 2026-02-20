//! OCI-compatible distribution interface for the module registry.
//!
//! Implements an OCI Distribution Spec-inspired push/pull API for WASM modules,
//! with manifest management, tag resolution, and verification pipelines.
//!
//! # Architecture
//!
//! ```text
//! OciRegistry
//! ├── repositories (name → tags → manifests → layers)
//! ├── push(reference, bytes, config) → manifest
//! ├── pull(reference) → bytes
//! └── verify(reference) → VerificationResult
//! ```



#![allow(missing_docs)]
use crate::module_registry::{content_hash, ModuleMetadata};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::SystemTime;

/// An OCI-style image reference: "repository:tag" or "repository@sha256:digest".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageReference {
    /// Repository name (e.g., "myorg/my-module").
    pub repository: String,
    /// Tag (e.g., "latest", "v1.0.0") or digest.
    pub tag: String,
}

impl ImageReference {
    /// Parse an image reference string.
    pub fn parse(reference: &str) -> Option<Self> {
        if let Some((repo, tag)) = reference.split_once(':') {
            Some(Self {
                repository: repo.to_string(),
                tag: tag.to_string(),
            })
        } else {
            Some(Self {
                repository: reference.to_string(),
                tag: "latest".to_string(),
            })
        }
    }

    /// Format as "repository:tag".
    pub fn to_string(&self) -> String {
        format!("{}:{}", self.repository, self.tag)
    }
}

/// OCI manifest for a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciManifest {
    /// Schema version (always 2).
    pub schema_version: u32,
    /// Media type.
    pub media_type: String,
    /// Configuration descriptor.
    pub config: OciDescriptor,
    /// Layer descriptors.
    pub layers: Vec<OciDescriptor>,
    /// Annotations.
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

/// OCI content descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciDescriptor {
    /// Media type of the referenced content.
    pub media_type: String,
    /// Content hash (sha256:...).
    pub digest: String,
    /// Size in bytes.
    pub size: u64,
    /// Optional annotations.
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

/// Configuration for pushing a module.
#[derive(Debug, Clone, Default)]
pub struct PushConfig {
    /// Module metadata.
    pub metadata: ModuleMetadata,
    /// Additional annotations.
    pub annotations: HashMap<String, String>,
    /// Maximum allowed module size.
    pub max_size: Option<u64>,
}

/// Result of a verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the module passed verification.
    pub passed: bool,
    /// Individual check results.
    pub checks: Vec<VerificationCheck>,
}

/// A single verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    /// Check name.
    pub name: String,
    /// Whether it passed.
    pub passed: bool,
    /// Details.
    pub detail: String,
}

/// Stored manifest with its data.
struct StoredManifest {
    manifest: OciManifest,
    wasm_bytes: Vec<u8>,
    #[allow(dead_code)] // Tracked for future manifest expiration policies
    pushed_at: SystemTime,
}

/// OCI errors.
#[derive(Debug, thiserror::Error)]
pub enum OciError {
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    #[error("Tag not found: {0}:{1}")]
    TagNotFound(String, String),
    #[error("Module too large: {size} bytes (max {max})")]
    ModuleTooLarge { size: u64, max: u64 },
    #[error("Invalid WASM: {0}")]
    InvalidWasm(String),
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// OCI-compatible module registry.
pub struct OciRegistry {
    /// repository → (tag → manifest)
    repositories: RwLock<HashMap<String, HashMap<String, StoredManifest>>>,
    /// Default max module size (100MB).
    default_max_size: u64,
}

impl OciRegistry {
    /// Create a new OCI registry.
    pub fn new() -> Self {
        Self {
            repositories: RwLock::new(HashMap::new()),
            default_max_size: 100 * 1024 * 1024,
        }
    }

    /// Push a WASM module to the registry.
    pub fn push(
        &self,
        reference: &ImageReference,
        wasm_bytes: &[u8],
        config: PushConfig,
    ) -> Result<OciManifest, OciError> {
        let size = wasm_bytes.len() as u64;
        let max = config.max_size.unwrap_or(self.default_max_size);
        if size > max {
            return Err(OciError::ModuleTooLarge { size, max });
        }

        // Validate WASM magic number
        if wasm_bytes.len() < 8 || &wasm_bytes[..4] != b"\x00asm" {
            return Err(OciError::InvalidWasm("Missing WASM magic number".to_string()));
        }

        let hash = content_hash(wasm_bytes);
        let digest = format!("sha256:{}", hash.0);

        let mut annotations = config.annotations;
        if !config.metadata.name.is_empty() {
            annotations.insert("org.opencontainers.image.title".to_string(), config.metadata.name.clone());
        }
        if !config.metadata.version.is_empty() {
            annotations.insert("org.opencontainers.image.version".to_string(), config.metadata.version.clone());
        }
        if !config.metadata.author.is_empty() {
            annotations.insert("org.opencontainers.image.authors".to_string(), config.metadata.author.clone());
        }

        let manifest = OciManifest {
            schema_version: 2,
            media_type: "application/vnd.oci.image.manifest.v1+json".to_string(),
            config: OciDescriptor {
                media_type: "application/vnd.wasm.config.v1+json".to_string(),
                digest: digest.clone(),
                size: 0,
                annotations: HashMap::new(),
            },
            layers: vec![OciDescriptor {
                media_type: "application/vnd.wasm.content.layer.v1+wasm".to_string(),
                digest,
                size,
                annotations: HashMap::new(),
            }],
            annotations,
        };

        let stored = StoredManifest {
            manifest: manifest.clone(),
            wasm_bytes: wasm_bytes.to_vec(),
            pushed_at: SystemTime::now(),
        };

        let mut repos = self.repositories.write()
            .map_err(|e| OciError::Internal(format!("lock poisoned: {e}")))?;
        repos
            .entry(reference.repository.clone())
            .or_default()
            .insert(reference.tag.clone(), stored);

        Ok(manifest)
    }

    /// Pull a WASM module from the registry.
    pub fn pull(&self, reference: &ImageReference) -> Result<(OciManifest, Vec<u8>), OciError> {
        let repos = self.repositories.read()
            .map_err(|e| OciError::Internal(format!("lock poisoned: {e}")))?;
        let tags = repos
            .get(&reference.repository)
            .ok_or_else(|| OciError::RepositoryNotFound(reference.repository.clone()))?;
        let stored = tags
            .get(&reference.tag)
            .ok_or_else(|| OciError::TagNotFound(reference.repository.clone(), reference.tag.clone()))?;

        Ok((stored.manifest.clone(), stored.wasm_bytes.clone()))
    }

    /// Get manifest without pulling the WASM bytes.
    pub fn get_manifest(&self, reference: &ImageReference) -> Result<OciManifest, OciError> {
        let repos = self.repositories.read()
            .map_err(|e| OciError::Internal(format!("lock poisoned: {e}")))?;
        let tags = repos
            .get(&reference.repository)
            .ok_or_else(|| OciError::RepositoryNotFound(reference.repository.clone()))?;
        let stored = tags
            .get(&reference.tag)
            .ok_or_else(|| OciError::TagNotFound(reference.repository.clone(), reference.tag.clone()))?;
        Ok(stored.manifest.clone())
    }

    /// List tags for a repository.
    pub fn list_tags(&self, repository: &str) -> Result<Vec<String>, OciError> {
        let repos = self.repositories.read()
            .map_err(|e| OciError::Internal(format!("lock poisoned: {e}")))?;
        let tags = repos
            .get(repository)
            .ok_or_else(|| OciError::RepositoryNotFound(repository.to_string()))?;
        Ok(tags.keys().cloned().collect())
    }

    /// List all repositories.
    pub fn list_repositories(&self) -> Vec<String> {
        let repos = match self.repositories.read() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        repos.keys().cloned().collect()
    }

    /// Delete a specific tag.
    pub fn delete(&self, reference: &ImageReference) -> Result<bool, OciError> {
        let mut repos = self.repositories.write()
            .map_err(|e| OciError::Internal(format!("lock poisoned: {e}")))?;
        let tags = repos
            .get_mut(&reference.repository)
            .ok_or_else(|| OciError::RepositoryNotFound(reference.repository.clone()))?;
        Ok(tags.remove(&reference.tag).is_some())
    }

    /// Verify a module against a set of checks.
    pub fn verify(&self, reference: &ImageReference) -> Result<VerificationResult, OciError> {
        let (manifest, wasm_bytes) = self.pull(reference)?;
        let mut checks = Vec::new();

        // Check 1: WASM magic number
        let magic_ok = wasm_bytes.len() >= 4 && &wasm_bytes[..4] == b"\x00asm";
        checks.push(VerificationCheck {
            name: "wasm_magic".to_string(),
            passed: magic_ok,
            detail: if magic_ok { "Valid WASM magic number" } else { "Invalid WASM magic" }.to_string(),
        });

        // Check 2: Content hash matches manifest
        let hash = content_hash(&wasm_bytes);
        let expected_digest = format!("sha256:{}", hash.0);
        let digest_ok = manifest.layers.first()
            .map_or(false, |l| l.digest == expected_digest);
        checks.push(VerificationCheck {
            name: "content_integrity".to_string(),
            passed: digest_ok,
            detail: if digest_ok { "Content hash matches manifest" } else { "Hash mismatch" }.to_string(),
        });

        // Check 3: Size matches
        let size_ok = manifest.layers.first()
            .map_or(false, |l| l.size == wasm_bytes.len() as u64);
        checks.push(VerificationCheck {
            name: "size_check".to_string(),
            passed: size_ok,
            detail: format!("Module size: {} bytes", wasm_bytes.len()),
        });

        // Check 4: Schema version
        let schema_ok = manifest.schema_version == 2;
        checks.push(VerificationCheck {
            name: "schema_version".to_string(),
            passed: schema_ok,
            detail: format!("Schema version: {}", manifest.schema_version),
        });

        let passed = checks.iter().all(|c| c.passed);
        Ok(VerificationResult { passed, checks })
    }
}

impl Default for OciRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ];

    fn test_reference() -> ImageReference {
        ImageReference::parse("myorg/hello:v1.0.0").unwrap()
    }

    #[test]
    fn test_parse_reference_with_tag() {
        let r = ImageReference::parse("myorg/hello:v1.0.0").unwrap();
        assert_eq!(r.repository, "myorg/hello");
        assert_eq!(r.tag, "v1.0.0");
    }

    #[test]
    fn test_parse_reference_without_tag() {
        let r = ImageReference::parse("myorg/hello").unwrap();
        assert_eq!(r.tag, "latest");
    }

    #[test]
    fn test_push_and_pull() {
        let registry = OciRegistry::new();
        let reference = test_reference();

        let manifest = registry.push(&reference, MINIMAL_WASM, PushConfig::default()).unwrap();
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.layers.len(), 1);

        let (pulled_manifest, bytes) = registry.pull(&reference).unwrap();
        assert_eq!(bytes, MINIMAL_WASM);
        assert_eq!(pulled_manifest.layers[0].size, MINIMAL_WASM.len() as u64);
    }

    #[test]
    fn test_push_with_metadata() {
        let registry = OciRegistry::new();
        let reference = test_reference();

        let config = PushConfig {
            metadata: ModuleMetadata {
                name: "hello".to_string(),
                version: "1.0.0".to_string(),
                author: "test".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let manifest = registry.push(&reference, MINIMAL_WASM, config).unwrap();
        assert_eq!(
            manifest.annotations.get("org.opencontainers.image.title").unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_pull_not_found() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        assert!(matches!(
            registry.pull(&reference),
            Err(OciError::RepositoryNotFound(_))
        ));
    }

    #[test]
    fn test_invalid_wasm() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        let bad_bytes = b"not wasm";
        assert!(matches!(
            registry.push(&reference, bad_bytes, PushConfig::default()),
            Err(OciError::InvalidWasm(_))
        ));
    }

    #[test]
    fn test_module_too_large() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        let config = PushConfig {
            max_size: Some(4),
            ..Default::default()
        };
        assert!(matches!(
            registry.push(&reference, MINIMAL_WASM, config),
            Err(OciError::ModuleTooLarge { .. })
        ));
    }

    #[test]
    fn test_list_tags() {
        let registry = OciRegistry::new();
        let r1 = ImageReference::parse("myorg/hello:v1.0").unwrap();
        let r2 = ImageReference::parse("myorg/hello:v2.0").unwrap();

        registry.push(&r1, MINIMAL_WASM, PushConfig::default()).unwrap();
        registry.push(&r2, MINIMAL_WASM, PushConfig::default()).unwrap();

        let tags = registry.list_tags("myorg/hello").unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"v1.0".to_string()));
        assert!(tags.contains(&"v2.0".to_string()));
    }

    #[test]
    fn test_list_repositories() {
        let registry = OciRegistry::new();
        let r1 = ImageReference::parse("org/mod-a:latest").unwrap();
        let r2 = ImageReference::parse("org/mod-b:latest").unwrap();

        registry.push(&r1, MINIMAL_WASM, PushConfig::default()).unwrap();
        registry.push(&r2, MINIMAL_WASM, PushConfig::default()).unwrap();

        let repos = registry.list_repositories();
        assert_eq!(repos.len(), 2);
    }

    #[test]
    fn test_delete() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        registry.push(&reference, MINIMAL_WASM, PushConfig::default()).unwrap();
        assert!(registry.delete(&reference).unwrap());
        assert!(registry.pull(&reference).is_err());
    }

    #[test]
    fn test_verify() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        registry.push(&reference, MINIMAL_WASM, PushConfig::default()).unwrap();

        let result = registry.verify(&reference).unwrap();
        assert!(result.passed);
        assert_eq!(result.checks.len(), 4);
        assert!(result.checks.iter().all(|c| c.passed));
    }

    #[test]
    fn test_get_manifest() {
        let registry = OciRegistry::new();
        let reference = test_reference();
        registry.push(&reference, MINIMAL_WASM, PushConfig::default()).unwrap();

        let manifest = registry.get_manifest(&reference).unwrap();
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.media_type, "application/vnd.oci.image.manifest.v1+json");
    }
}
