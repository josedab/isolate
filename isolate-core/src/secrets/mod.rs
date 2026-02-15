//! Secrets Management
//!
//! Secure secret injection and management:
//! - Integration with HashiCorp Vault, AWS Secrets Manager, etc.
//! - Automatic secret rotation
//! - In-memory secret protection
//! - Audit logging for secret access

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

pub mod compliance;
pub mod rotation;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Secret reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretRef {
    /// Secret provider.
    pub provider: SecretProvider,
    /// Secret path/name.
    pub path: String,
    /// Secret key (for key-value secrets).
    pub key: Option<String>,
    /// Version (None = latest).
    pub version: Option<String>,
}

impl SecretRef {
    /// Create a new secret reference.
    pub fn new(provider: SecretProvider, path: impl Into<String>) -> Self {
        Self { provider, path: path.into(), key: None, version: None }
    }

    /// With specific key.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// With specific version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

/// Secret provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecretProvider {
    /// HashiCorp Vault.
    Vault { address: String, mount: String },
    /// AWS Secrets Manager.
    AwsSecretsManager { region: String },
    /// Google Cloud Secret Manager.
    GcpSecretManager { project: String },
    /// Azure Key Vault.
    AzureKeyVault { vault_name: String },
    /// Kubernetes Secret.
    Kubernetes { namespace: String },
    /// Environment variable.
    Environment,
    /// Local file.
    File { base_path: String },
    /// In-memory (for testing).
    InMemory,
}

/// Secret value.
#[derive(Clone)]
pub struct SecretValue {
    /// Raw bytes.
    value: Vec<u8>,
    /// Metadata.
    metadata: SecretMetadata,
}

impl SecretValue {
    /// Create new secret value.
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self { value: value.into(), metadata: SecretMetadata::default() }
    }

    /// Get value as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    /// Get value as string.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.value)
    }

    /// Get metadata.
    pub fn metadata(&self) -> &SecretMetadata {
        &self.metadata
    }

    /// Zeroize on drop.
    fn zeroize(&mut self) {
        for byte in &mut self.value {
            *byte = 0;
        }
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.zeroize();
    }
}

// Don't expose secret value in debug
impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretValue")
            .field("length", &self.value.len())
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Secret metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretMetadata {
    /// Version.
    pub version: Option<String>,
    /// Created at.
    pub created_at: Option<SystemTime>,
    /// Expires at.
    pub expires_at: Option<SystemTime>,
    /// Rotation interval.
    pub rotation_interval: Option<Duration>,
    /// Custom metadata.
    pub custom: HashMap<String, String>,
}

/// Secret access event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretAccessEvent {
    /// Timestamp.
    pub timestamp: SystemTime,
    /// Secret reference.
    pub secret_ref: SecretRef,
    /// Accessor (sandbox ID or user).
    pub accessor: String,
    /// Access type.
    pub access_type: SecretAccessType,
    /// Success.
    pub success: bool,
    /// Error if any.
    pub error: Option<String>,
}

/// Secret access type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecretAccessType {
    Read,
    Write,
    Delete,
    Rotate,
    List,
}

/// Secret manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    /// Default provider.
    pub default_provider: SecretProvider,
    /// Cache TTL.
    pub cache_ttl: Duration,
    /// Enable audit logging.
    pub audit_enabled: bool,
    /// Allowed providers.
    pub allowed_providers: Vec<SecretProvider>,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            default_provider: SecretProvider::InMemory,
            cache_ttl: Duration::from_secs(300),
            audit_enabled: true,
            allowed_providers: vec![SecretProvider::InMemory, SecretProvider::Environment],
        }
    }
}

/// Cached secret.
struct CachedSecret {
    value: SecretValue,
    fetched_at: SystemTime,
    expires_at: SystemTime,
}

/// Secrets manager.
pub struct SecretsManager {
    config: SecretsConfig,
    cache: HashMap<SecretRef, CachedSecret>,
    memory_store: HashMap<String, SecretValue>,
    audit_log: Vec<SecretAccessEvent>,
}

impl SecretsManager {
    /// Create new secrets manager.
    pub fn new(config: SecretsConfig) -> Self {
        Self { config, cache: HashMap::new(), memory_store: HashMap::new(), audit_log: Vec::new() }
    }

    /// Get a secret.
    pub fn get(
        &mut self,
        secret_ref: &SecretRef,
        accessor: &str,
    ) -> Result<SecretValue, SecretsError> {
        // Check if provider is allowed
        if !self.is_provider_allowed(&secret_ref.provider) {
            self.log_access(
                secret_ref,
                accessor,
                SecretAccessType::Read,
                false,
                Some("Provider not allowed"),
            );
            return Err(SecretsError::ProviderNotAllowed);
        }

        // Check cache
        if let Some(cached) = self.cache.get(secret_ref) {
            if cached.expires_at > SystemTime::now() {
                let value = cached.value.clone();
                self.log_access(secret_ref, accessor, SecretAccessType::Read, true, None);
                return Ok(value);
            }
        }

        // Fetch from provider
        let value = self.fetch_from_provider(secret_ref)?;

        // Cache the value
        let now = SystemTime::now();
        self.cache.insert(
            secret_ref.clone(),
            CachedSecret {
                value: value.clone(),
                fetched_at: now,
                expires_at: now + self.config.cache_ttl,
            },
        );

        self.log_access(secret_ref, accessor, SecretAccessType::Read, true, None);
        Ok(value)
    }

    /// Set a secret (for InMemory provider).
    pub fn set(
        &mut self,
        path: &str,
        value: SecretValue,
        accessor: &str,
    ) -> Result<(), SecretsError> {
        let secret_ref = SecretRef::new(SecretProvider::InMemory, path);
        self.memory_store.insert(path.to_string(), value);
        self.log_access(&secret_ref, accessor, SecretAccessType::Write, true, None);

        // Invalidate cache
        self.cache.remove(&secret_ref);

        Ok(())
    }

    /// Delete a secret.
    pub fn delete(&mut self, secret_ref: &SecretRef, accessor: &str) -> Result<(), SecretsError> {
        match &secret_ref.provider {
            SecretProvider::InMemory => {
                self.memory_store.remove(&secret_ref.path);
            }
            _ => {
                return Err(SecretsError::OperationNotSupported);
            }
        }

        self.cache.remove(secret_ref);
        self.log_access(secret_ref, accessor, SecretAccessType::Delete, true, None);
        Ok(())
    }

    /// List secrets (paths only).
    pub fn list(
        &self,
        provider: &SecretProvider,
        accessor: &str,
    ) -> Result<Vec<String>, SecretsError> {
        let paths = match provider {
            SecretProvider::InMemory => self.memory_store.keys().cloned().collect(),
            _ => {
                return Err(SecretsError::OperationNotSupported);
            }
        };

        let secret_ref = SecretRef::new(provider.clone(), "");
        // Note: Can't log access here since we don't have mutable ref
        let _ = accessor;
        let _ = secret_ref;

        Ok(paths)
    }

    /// Rotate a secret.
    pub fn rotate(
        &mut self,
        secret_ref: &SecretRef,
        new_value: SecretValue,
        accessor: &str,
    ) -> Result<(), SecretsError> {
        match &secret_ref.provider {
            SecretProvider::InMemory => {
                self.memory_store.insert(secret_ref.path.clone(), new_value);
            }
            _ => {
                return Err(SecretsError::OperationNotSupported);
            }
        }

        self.cache.remove(secret_ref);
        self.log_access(secret_ref, accessor, SecretAccessType::Rotate, true, None);
        Ok(())
    }

    /// Get audit log.
    pub fn audit_log(&self) -> &[SecretAccessEvent] {
        &self.audit_log
    }

    /// Clear audit log.
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Invalidate cache.
    pub fn invalidate_cache(&mut self) {
        self.cache.clear();
    }

    /// Invalidate specific secret.
    pub fn invalidate(&mut self, secret_ref: &SecretRef) {
        self.cache.remove(secret_ref);
    }

    fn fetch_from_provider(&self, secret_ref: &SecretRef) -> Result<SecretValue, SecretsError> {
        match &secret_ref.provider {
            SecretProvider::InMemory => self
                .memory_store
                .get(&secret_ref.path)
                .cloned()
                .ok_or_else(|| SecretsError::NotFound(secret_ref.path.clone())),
            SecretProvider::Environment => std::env::var(&secret_ref.path)
                .map(|v| SecretValue::new(v.into_bytes()))
                .map_err(|_| SecretsError::NotFound(secret_ref.path.clone())),
            SecretProvider::File { base_path } => {
                let full_path = format!("{}/{}", base_path, secret_ref.path);
                std::fs::read(&full_path)
                    .map(SecretValue::new)
                    .map_err(|e| SecretsError::ProviderError(e.to_string()))
            }
            SecretProvider::Vault { address, mount } => {
                // Would use vault client
                Err(SecretsError::ProviderError(format!(
                    "Vault provider not implemented: {} mount {}",
                    address, mount
                )))
            }
            SecretProvider::AwsSecretsManager { region } => {
                // Would use AWS SDK
                Err(SecretsError::ProviderError(format!(
                    "AWS provider not implemented: region {}",
                    region
                )))
            }
            SecretProvider::GcpSecretManager { project } => Err(SecretsError::ProviderError(
                format!("GCP provider not implemented: project {}", project),
            )),
            SecretProvider::AzureKeyVault { vault_name } => Err(SecretsError::ProviderError(
                format!("Azure provider not implemented: vault {}", vault_name),
            )),
            SecretProvider::Kubernetes { namespace } => Err(SecretsError::ProviderError(format!(
                "K8s provider not implemented: namespace {}",
                namespace
            ))),
        }
    }

    fn is_provider_allowed(&self, provider: &SecretProvider) -> bool {
        self.config
            .allowed_providers
            .iter()
            .any(|p| std::mem::discriminant(p) == std::mem::discriminant(provider))
    }

    fn log_access(
        &mut self,
        secret_ref: &SecretRef,
        accessor: &str,
        access_type: SecretAccessType,
        success: bool,
        error: Option<&str>,
    ) {
        if !self.config.audit_enabled {
            return;
        }

        self.audit_log.push(SecretAccessEvent {
            timestamp: SystemTime::now(),
            secret_ref: secret_ref.clone(),
            accessor: accessor.to_string(),
            access_type,
            success,
            error: error.map(|s| s.to_string()),
        });
    }
}

impl Default for SecretsManager {
    fn default() -> Self {
        Self::new(SecretsConfig::default())
    }
}

/// Secrets error.
#[derive(Debug, Clone)]
pub enum SecretsError {
    /// Secret not found.
    NotFound(String),
    /// Provider not allowed.
    ProviderNotAllowed,
    /// Provider error.
    ProviderError(String),
    /// Operation not supported.
    OperationNotSupported,
    /// Access denied.
    AccessDenied,
    /// Secret expired.
    Expired,
}

impl std::fmt::Display for SecretsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Secret not found: {}", path),
            Self::ProviderNotAllowed => write!(f, "Secret provider not allowed"),
            Self::ProviderError(e) => write!(f, "Provider error: {}", e),
            Self::OperationNotSupported => write!(f, "Operation not supported"),
            Self::AccessDenied => write!(f, "Access denied"),
            Self::Expired => write!(f, "Secret expired"),
        }
    }
}

impl std::error::Error for SecretsError {}

/// Secret injector for sandboxes.
pub struct SecretInjector {
    manager: Arc<std::sync::RwLock<SecretsManager>>,
    sandbox_id: String,
}

impl SecretInjector {
    /// Create new injector.
    pub fn new(
        manager: Arc<std::sync::RwLock<SecretsManager>>,
        sandbox_id: impl Into<String>,
    ) -> Self {
        Self { manager, sandbox_id: sandbox_id.into() }
    }

    /// Inject secrets as environment variables.
    pub fn inject_env(
        &self,
        mappings: &[(SecretRef, String)],
    ) -> Result<HashMap<String, String>, SecretsError> {
        let mut env = HashMap::new();
        let mut manager = self
            .manager
            .write()
            .map_err(|_| SecretsError::ProviderError("Lock error".to_string()))?;

        for (secret_ref, env_var) in mappings {
            let value = manager.get(secret_ref, &self.sandbox_id)?;
            let value_str = value
                .as_str()
                .map_err(|_| SecretsError::ProviderError("Invalid UTF-8".to_string()))?;
            env.insert(env_var.clone(), value_str.to_string());
        }

        Ok(env)
    }

    /// Get a single secret.
    pub fn get(&self, secret_ref: &SecretRef) -> Result<SecretValue, SecretsError> {
        let mut manager = self
            .manager
            .write()
            .map_err(|_| SecretsError::ProviderError("Lock error".to_string()))?;
        manager.get(secret_ref, &self.sandbox_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_value_creation() {
        let secret = SecretValue::new(b"my-secret".to_vec());
        assert_eq!(secret.as_bytes(), b"my-secret");
        assert_eq!(secret.as_str().unwrap(), "my-secret");
    }

    #[test]
    fn test_secrets_manager_inmemory() {
        let mut manager = SecretsManager::default();

        // Set a secret
        manager.set("api-key", SecretValue::new(b"secret123".to_vec()), "test").unwrap();

        // Get the secret
        let secret_ref = SecretRef::new(SecretProvider::InMemory, "api-key");
        let value = manager.get(&secret_ref, "test").unwrap();

        assert_eq!(value.as_str().unwrap(), "secret123");
    }

    #[test]
    fn test_secret_not_found() {
        let mut manager = SecretsManager::default();
        let secret_ref = SecretRef::new(SecretProvider::InMemory, "nonexistent");

        let result = manager.get(&secret_ref, "test");
        assert!(matches!(result, Err(SecretsError::NotFound(_))));
    }

    #[test]
    fn test_secret_delete() {
        let mut manager = SecretsManager::default();

        manager.set("to-delete", SecretValue::new(b"value".to_vec()), "test").unwrap();

        let secret_ref = SecretRef::new(SecretProvider::InMemory, "to-delete");
        manager.delete(&secret_ref, "test").unwrap();

        let result = manager.get(&secret_ref, "test");
        assert!(matches!(result, Err(SecretsError::NotFound(_))));
    }

    #[test]
    fn test_secret_rotation() {
        let mut manager = SecretsManager::default();

        manager.set("rotate-me", SecretValue::new(b"old-value".to_vec()), "test").unwrap();

        let secret_ref = SecretRef::new(SecretProvider::InMemory, "rotate-me");
        manager.rotate(&secret_ref, SecretValue::new(b"new-value".to_vec()), "test").unwrap();

        let value = manager.get(&secret_ref, "test").unwrap();
        assert_eq!(value.as_str().unwrap(), "new-value");
    }

    #[test]
    fn test_secret_list() {
        let mut manager = SecretsManager::default();

        manager.set("secret1", SecretValue::new(b"v1".to_vec()), "test").unwrap();
        manager.set("secret2", SecretValue::new(b"v2".to_vec()), "test").unwrap();

        let paths = manager.list(&SecretProvider::InMemory, "test").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"secret1".to_string()));
        assert!(paths.contains(&"secret2".to_string()));
    }

    #[test]
    fn test_audit_logging() {
        let mut manager =
            SecretsManager::new(SecretsConfig { audit_enabled: true, ..Default::default() });

        manager.set("audited", SecretValue::new(b"value".to_vec()), "user1").unwrap();
        let secret_ref = SecretRef::new(SecretProvider::InMemory, "audited");
        manager.get(&secret_ref, "user2").unwrap();

        let log = manager.audit_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].access_type, SecretAccessType::Write);
        assert_eq!(log[1].access_type, SecretAccessType::Read);
    }

    #[test]
    fn test_provider_not_allowed() {
        let config = SecretsConfig {
            allowed_providers: vec![SecretProvider::InMemory],
            ..Default::default()
        };
        let mut manager = SecretsManager::new(config);

        let secret_ref = SecretRef::new(
            SecretProvider::Vault {
                address: "http://vault:8200".to_string(),
                mount: "secret".to_string(),
            },
            "my-secret",
        );

        let result = manager.get(&secret_ref, "test");
        assert!(matches!(result, Err(SecretsError::ProviderNotAllowed)));
    }

    #[test]
    fn test_secret_caching() {
        let mut manager = SecretsManager::new(SecretsConfig {
            cache_ttl: Duration::from_secs(300),
            ..Default::default()
        });

        manager.set("cached", SecretValue::new(b"value".to_vec()), "test").unwrap();
        let secret_ref = SecretRef::new(SecretProvider::InMemory, "cached");

        // First fetch populates cache
        manager.get(&secret_ref, "test").unwrap();

        // Delete from memory store
        manager.memory_store.remove("cached");

        // Should still get from cache
        let value = manager.get(&secret_ref, "test").unwrap();
        assert_eq!(value.as_str().unwrap(), "value");
    }

    #[test]
    fn test_cache_invalidation() {
        let mut manager = SecretsManager::default();

        manager.set("to-invalidate", SecretValue::new(b"v1".to_vec()), "test").unwrap();
        let secret_ref = SecretRef::new(SecretProvider::InMemory, "to-invalidate");
        manager.get(&secret_ref, "test").unwrap();

        manager.invalidate(&secret_ref);
        manager.set("to-invalidate", SecretValue::new(b"v2".to_vec()), "test").unwrap();

        let value = manager.get(&secret_ref, "test").unwrap();
        assert_eq!(value.as_str().unwrap(), "v2");
    }

    #[test]
    fn test_secret_ref_with_key() {
        let secret_ref =
            SecretRef::new(SecretProvider::InMemory, "config").key("database_url").version("v1");

        assert_eq!(secret_ref.key, Some("database_url".to_string()));
        assert_eq!(secret_ref.version, Some("v1".to_string()));
    }

    #[test]
    fn test_env_provider() {
        std::env::set_var("TEST_SECRET_VAR", "env-value");

        let config = SecretsConfig {
            allowed_providers: vec![SecretProvider::Environment],
            ..Default::default()
        };
        let mut manager = SecretsManager::new(config);

        let secret_ref = SecretRef::new(SecretProvider::Environment, "TEST_SECRET_VAR");
        let value = manager.get(&secret_ref, "test").unwrap();

        assert_eq!(value.as_str().unwrap(), "env-value");

        std::env::remove_var("TEST_SECRET_VAR");
    }

    #[test]
    fn test_secret_injector() {
        let manager = Arc::new(std::sync::RwLock::new(SecretsManager::default()));

        {
            let mut m = manager.write().unwrap();
            m.set("api-key", SecretValue::new(b"key123".to_vec()), "setup").unwrap();
            m.set("api-secret", SecretValue::new(b"secret456".to_vec()), "setup").unwrap();
        }

        let injector = SecretInjector::new(manager, "sandbox-1");

        let mappings = vec![
            (SecretRef::new(SecretProvider::InMemory, "api-key"), "API_KEY".to_string()),
            (SecretRef::new(SecretProvider::InMemory, "api-secret"), "API_SECRET".to_string()),
        ];

        let env = injector.inject_env(&mappings).unwrap();
        assert_eq!(env.get("API_KEY").unwrap(), "key123");
        assert_eq!(env.get("API_SECRET").unwrap(), "secret456");
    }
}
