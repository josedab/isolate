//! Event-driven trigger system for invoking sandboxes.
//!
//! Provides trigger definitions, a registry for managing them, and event types
//! for representing incoming trigger invocations.

#![allow(missing_docs)]//!
//! # Example
//!
//! ```rust
//! use isolate_core::engine::triggers::{
//!     HttpMethod, TriggerDefinition, TriggerKind, TriggerRegistry,
//! };
//!
//! let mut registry = TriggerRegistry::new();
//!
//! let trigger = TriggerDefinition::builder()
//!     .id("my-http-trigger")
//!     .name("My HTTP Trigger")
//!     .kind(TriggerKind::Http {
//!         path: "/api/run".to_string(),
//!         methods: vec![HttpMethod::Post],
//!     })
//!     .module_hash("abc123")
//!     .build()
//!     .unwrap();
//!
//! registry.register(trigger).unwrap();
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// HttpMethod
// ---------------------------------------------------------------------------

/// HTTP methods that a trigger can match.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    /// Matches any HTTP method.
    Any,
}

// ---------------------------------------------------------------------------
// TriggerKind
// ---------------------------------------------------------------------------

/// The kind of event that activates a trigger.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TriggerKind {
    /// Triggered by an incoming HTTP request matching the path and methods.
    Http {
        path: String,
        methods: Vec<HttpMethod>,
    },
    /// Triggered on a cron schedule (e.g. `"0 * * * *"`).
    Cron { schedule: String },
    /// Triggered at a fixed interval.
    Timer { interval: Duration },
    /// Triggered by an incoming webhook, optionally verified with a shared secret.
    Webhook { secret: Option<String> },
}

// ---------------------------------------------------------------------------
// TriggerConfig
// ---------------------------------------------------------------------------

/// Per-trigger overrides applied when creating the sandbox.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TriggerConfig {
    /// Optional memory limit override (bytes).
    pub memory_limit: Option<usize>,
    /// Optional fuel (CPU) limit override.
    pub fuel: Option<u64>,
    /// Optional execution timeout override.
    pub timeout: Option<Duration>,
    /// Extra environment variables injected into the sandbox.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// TriggerDefinition
// ---------------------------------------------------------------------------

/// A registered trigger that maps an event source to a WASM module invocation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TriggerDefinition {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What kind of event activates this trigger.
    pub kind: TriggerKind,
    /// Hash of the WASM module to invoke.
    pub module_hash: String,
    /// Per-trigger sandbox configuration overrides.
    pub config_overrides: TriggerConfig,
    /// Whether this trigger is currently enabled.
    pub enabled: bool,
    /// Maximum number of concurrent invocations from this trigger.
    pub max_concurrent: usize,
}

/// Builder for [`TriggerDefinition`].
#[derive(Debug, Default)]
#[must_use = "builders do nothing unless you call .build()"]
pub struct TriggerDefinitionBuilder {
    id: Option<String>,
    name: Option<String>,
    kind: Option<TriggerKind>,
    module_hash: Option<String>,
    config_overrides: TriggerConfig,
    enabled: bool,
    max_concurrent: usize,
}

impl TriggerDefinition {
    /// Create a new builder.
    pub fn builder() -> TriggerDefinitionBuilder {
        TriggerDefinitionBuilder::new()
    }
}

impl TriggerDefinitionBuilder {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            enabled: true,
            max_concurrent: 1,
            ..Default::default()
        }
    }

    /// Set the trigger id.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the human-readable name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the trigger kind.
    pub fn kind(mut self, kind: TriggerKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Set the WASM module hash to invoke.
    pub fn module_hash(mut self, hash: impl Into<String>) -> Self {
        self.module_hash = Some(hash.into());
        self
    }

    /// Set per-trigger configuration overrides.
    pub fn config_overrides(mut self, config: TriggerConfig) -> Self {
        self.config_overrides = config;
        self
    }

    /// Set whether the trigger is enabled (default: `true`).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the maximum concurrent invocations (default: `1`).
    pub fn max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Build the [`TriggerDefinition`].
    ///
    /// Returns an error if required fields (`id`, `name`, `kind`, `module_hash`)
    /// are missing, or if `max_concurrent` is zero.
    pub fn build(self) -> Result<TriggerDefinition> {
        let id = self
            .id
            .ok_or_else(|| Error::InvalidConfig("trigger id is required".into()))?;
        let name = self
            .name
            .ok_or_else(|| Error::InvalidConfig("trigger name is required".into()))?;
        let kind = self
            .kind
            .ok_or_else(|| Error::InvalidConfig("trigger kind is required".into()))?;
        let module_hash = self
            .module_hash
            .ok_or_else(|| Error::InvalidConfig("module_hash is required".into()))?;

        if self.max_concurrent == 0 {
            return Err(Error::InvalidConfig(
                "max_concurrent must be at least 1".into(),
            ));
        }

        // Validate cron schedule has exactly 5 fields.
        if let TriggerKind::Cron { ref schedule } = kind {
            let fields: Vec<&str> = schedule.split_whitespace().collect();
            if fields.len() != 5 {
                return Err(Error::InvalidConfig(format!(
                    "cron schedule must have exactly 5 fields, got {}",
                    fields.len()
                )));
            }
        }

        Ok(TriggerDefinition {
            id,
            name,
            kind,
            module_hash,
            config_overrides: self.config_overrides,
            enabled: self.enabled,
            max_concurrent: self.max_concurrent,
        })
    }
}

// ---------------------------------------------------------------------------
// TriggerEvent
// ---------------------------------------------------------------------------

/// An incoming trigger invocation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEvent {
    /// Identifier of the trigger that fired.
    pub trigger_id: String,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

impl TriggerEvent {
    /// Create a new event for the given trigger.
    pub fn new(trigger_id: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            trigger_id: trigger_id.into(),
            payload,
            metadata: HashMap::new(),
            timestamp: Utc::now(),
        }
    }

    /// Attach a metadata key-value pair.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// TriggerRegistry
// ---------------------------------------------------------------------------

/// Registry for managing trigger definitions.
pub struct TriggerRegistry {
    triggers: HashMap<String, TriggerDefinition>,
}

impl TriggerRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            triggers: HashMap::new(),
        }
    }

    /// Register a trigger definition.
    ///
    /// Returns an error if a trigger with the same id is already registered.
    pub fn register(&mut self, definition: TriggerDefinition) -> Result<()> {
        if self.triggers.contains_key(&definition.id) {
            return Err(Error::InvalidConfig(format!(
                "trigger '{}' is already registered",
                definition.id
            )));
        }
        self.triggers.insert(definition.id.clone(), definition);
        Ok(())
    }

    /// Unregister a trigger by id. Returns `true` if it was present.
    pub fn unregister(&mut self, id: &str) -> bool {
        self.triggers.remove(id).is_some()
    }

    /// Look up a trigger by id.
    pub fn get(&self, id: &str) -> Option<&TriggerDefinition> {
        self.triggers.get(id)
    }

    /// List all registered triggers.
    pub fn list(&self) -> Vec<&TriggerDefinition> {
        self.triggers.values().collect()
    }

    /// Find all enabled triggers that match the given HTTP path and method.
    pub fn match_http(&self, path: &str, method: &HttpMethod) -> Vec<&TriggerDefinition> {
        self.triggers
            .values()
            .filter(|t| {
                if !t.enabled {
                    return false;
                }
                match &t.kind {
                    TriggerKind::Http {
                        path: trigger_path,
                        methods,
                    } => {
                        trigger_path == path
                            && (methods.contains(&HttpMethod::Any)
                                || methods.contains(method))
                    }
                    _ => false,
                }
            })
            .collect()
    }

    /// List all enabled triggers.
    pub fn enabled_triggers(&self) -> Vec<&TriggerDefinition> {
        self.triggers.values().filter(|t| t.enabled).collect()
    }
}

impl Default for TriggerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_http_trigger(id: &str, path: &str, methods: Vec<HttpMethod>) -> TriggerDefinition {
        TriggerDefinition::builder()
            .id(id)
            .name(format!("trigger-{id}"))
            .kind(TriggerKind::Http {
                path: path.to_string(),
                methods,
            })
            .module_hash("deadbeef")
            .build()
            .unwrap()
    }

    // -- Registration --------------------------------------------------------

    #[test]
    fn test_register_and_get() {
        let mut reg = TriggerRegistry::new();
        let trigger = sample_http_trigger("t1", "/api", vec![HttpMethod::Get]);
        reg.register(trigger).unwrap();

        let found = reg.get("t1").unwrap();
        assert_eq!(found.id, "t1");
        assert_eq!(found.module_hash, "deadbeef");
    }

    #[test]
    fn test_register_duplicate_errors() {
        let mut reg = TriggerRegistry::new();
        let t1 = sample_http_trigger("dup", "/a", vec![HttpMethod::Get]);
        let t2 = sample_http_trigger("dup", "/b", vec![HttpMethod::Post]);
        reg.register(t1).unwrap();
        assert!(reg.register(t2).is_err());
    }

    #[test]
    fn test_unregister() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("rm", "/x", vec![HttpMethod::Get]))
            .unwrap();
        assert!(reg.unregister("rm"));
        assert!(!reg.unregister("rm"));
        assert!(reg.get("rm").is_none());
    }

    #[test]
    fn test_list_triggers() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("a", "/a", vec![HttpMethod::Get]))
            .unwrap();
        reg.register(sample_http_trigger("b", "/b", vec![HttpMethod::Post]))
            .unwrap();
        assert_eq!(reg.list().len(), 2);
    }

    // -- HTTP matching -------------------------------------------------------

    #[test]
    fn test_match_http_exact() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("h1", "/run", vec![HttpMethod::Post]))
            .unwrap();
        reg.register(sample_http_trigger("h2", "/other", vec![HttpMethod::Post]))
            .unwrap();

        let matches = reg.match_http("/run", &HttpMethod::Post);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "h1");
    }

    #[test]
    fn test_match_http_method_mismatch() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("h1", "/run", vec![HttpMethod::Post]))
            .unwrap();

        let matches = reg.match_http("/run", &HttpMethod::Get);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_http_any_method() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("h1", "/any", vec![HttpMethod::Any]))
            .unwrap();

        assert_eq!(reg.match_http("/any", &HttpMethod::Get).len(), 1);
        assert_eq!(reg.match_http("/any", &HttpMethod::Delete).len(), 1);
    }

    #[test]
    fn test_match_http_multiple_methods() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger(
            "h1",
            "/multi",
            vec![HttpMethod::Get, HttpMethod::Post],
        ))
        .unwrap();

        assert_eq!(reg.match_http("/multi", &HttpMethod::Get).len(), 1);
        assert_eq!(reg.match_http("/multi", &HttpMethod::Post).len(), 1);
        assert!(reg.match_http("/multi", &HttpMethod::Delete).is_empty());
    }

    #[test]
    fn test_match_http_skips_disabled() {
        let mut reg = TriggerRegistry::new();
        let t = TriggerDefinition::builder()
            .id("dis")
            .name("disabled trigger")
            .kind(TriggerKind::Http {
                path: "/disabled".to_string(),
                methods: vec![HttpMethod::Get],
            })
            .module_hash("hash")
            .enabled(false)
            .build()
            .unwrap();
        reg.register(t).unwrap();

        assert!(reg.match_http("/disabled", &HttpMethod::Get).is_empty());
    }

    // -- Cron validation -----------------------------------------------------

    #[test]
    fn test_cron_valid_schedule() {
        let t = TriggerDefinition::builder()
            .id("c1")
            .name("cron trigger")
            .kind(TriggerKind::Cron {
                schedule: "0 * * * *".to_string(),
            })
            .module_hash("hash")
            .build();
        assert!(t.is_ok());
    }

    #[test]
    fn test_cron_invalid_schedule_too_few_fields() {
        let t = TriggerDefinition::builder()
            .id("c2")
            .name("bad cron")
            .kind(TriggerKind::Cron {
                schedule: "0 *".to_string(),
            })
            .module_hash("hash")
            .build();
        assert!(t.is_err());
    }

    #[test]
    fn test_cron_invalid_schedule_too_many_fields() {
        let t = TriggerDefinition::builder()
            .id("c3")
            .name("bad cron")
            .kind(TriggerKind::Cron {
                schedule: "0 * * * * *".to_string(),
            })
            .module_hash("hash")
            .build();
        assert!(t.is_err());
    }

    // -- Enable / Disable ----------------------------------------------------

    #[test]
    fn test_enabled_triggers() {
        let mut reg = TriggerRegistry::new();
        reg.register(sample_http_trigger("e1", "/a", vec![HttpMethod::Get]))
            .unwrap();

        let disabled = TriggerDefinition::builder()
            .id("e2")
            .name("disabled")
            .kind(TriggerKind::Webhook { secret: None })
            .module_hash("hash")
            .enabled(false)
            .build()
            .unwrap();
        reg.register(disabled).unwrap();

        let enabled = reg.enabled_triggers();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "e1");
    }

    // -- Max concurrent ------------------------------------------------------

    #[test]
    fn test_max_concurrent_default() {
        let t = sample_http_trigger("mc", "/mc", vec![HttpMethod::Get]);
        assert_eq!(t.max_concurrent, 1);
    }

    #[test]
    fn test_max_concurrent_custom() {
        let t = TriggerDefinition::builder()
            .id("mc2")
            .name("high concurrency")
            .kind(TriggerKind::Timer {
                interval: Duration::from_secs(60),
            })
            .module_hash("hash")
            .max_concurrent(10)
            .build()
            .unwrap();
        assert_eq!(t.max_concurrent, 10);
    }

    #[test]
    fn test_max_concurrent_zero_rejected() {
        let t = TriggerDefinition::builder()
            .id("mc0")
            .name("zero")
            .kind(TriggerKind::Webhook { secret: None })
            .module_hash("hash")
            .max_concurrent(0)
            .build();
        assert!(t.is_err());
    }

    // -- TriggerEvent --------------------------------------------------------

    #[test]
    fn test_event_creation() {
        let event = TriggerEvent::new("t1", b"hello".to_vec());
        assert_eq!(event.trigger_id, "t1");
        assert_eq!(event.payload, b"hello");
        assert!(event.metadata.is_empty());
    }

    #[test]
    fn test_event_with_metadata() {
        let event = TriggerEvent::new("t1", vec![])
            .with_metadata("source", "test")
            .with_metadata("request_id", "abc-123");
        assert_eq!(event.metadata.get("source").unwrap(), "test");
        assert_eq!(event.metadata.get("request_id").unwrap(), "abc-123");
    }

    // -- Builder pattern -----------------------------------------------------

    #[test]
    fn test_builder_missing_id() {
        let res = TriggerDefinition::builder()
            .name("no id")
            .kind(TriggerKind::Webhook { secret: None })
            .module_hash("hash")
            .build();
        assert!(res.is_err());
    }

    #[test]
    fn test_builder_missing_name() {
        let res = TriggerDefinition::builder()
            .id("no-name")
            .kind(TriggerKind::Webhook { secret: None })
            .module_hash("hash")
            .build();
        assert!(res.is_err());
    }

    #[test]
    fn test_builder_missing_kind() {
        let res = TriggerDefinition::builder()
            .id("no-kind")
            .name("no kind")
            .module_hash("hash")
            .build();
        assert!(res.is_err());
    }

    #[test]
    fn test_builder_missing_module_hash() {
        let res = TriggerDefinition::builder()
            .id("no-hash")
            .name("no hash")
            .kind(TriggerKind::Webhook { secret: None })
            .build();
        assert!(res.is_err());
    }

    #[test]
    fn test_builder_with_config_overrides() {
        let config = TriggerConfig {
            memory_limit: Some(256 * 1024 * 1024),
            fuel: Some(5_000_000),
            timeout: Some(Duration::from_secs(30)),
            env: HashMap::from([("KEY".into(), "VALUE".into())]),
        };

        let t = TriggerDefinition::builder()
            .id("cfg")
            .name("with config")
            .kind(TriggerKind::Webhook { secret: None })
            .module_hash("hash")
            .config_overrides(config)
            .build()
            .unwrap();

        assert_eq!(t.config_overrides.memory_limit, Some(256 * 1024 * 1024));
        assert_eq!(t.config_overrides.fuel, Some(5_000_000));
        assert_eq!(t.config_overrides.timeout, Some(Duration::from_secs(30)));
        assert_eq!(t.config_overrides.env.get("KEY").unwrap(), "VALUE");
    }

    #[test]
    fn test_builder_webhook_with_secret() {
        let t = TriggerDefinition::builder()
            .id("wh")
            .name("webhook")
            .kind(TriggerKind::Webhook {
                secret: Some("s3cret".to_string()),
            })
            .module_hash("hash")
            .build()
            .unwrap();

        let TriggerKind::Webhook { secret } = &t.kind else {
            unreachable!("expected Webhook kind");
        };
        assert_eq!(secret.as_deref(), Some("s3cret"));
    }

    #[test]
    fn test_timer_trigger() {
        let t = TriggerDefinition::builder()
            .id("timer")
            .name("timer trigger")
            .kind(TriggerKind::Timer {
                interval: Duration::from_secs(120),
            })
            .module_hash("hash")
            .build()
            .unwrap();

        let TriggerKind::Timer { interval } = &t.kind else {
            unreachable!("expected Timer kind");
        };
        assert_eq!(*interval, Duration::from_secs(120));
    }

    #[test]
    fn test_registry_default() {
        let reg = TriggerRegistry::default();
        assert!(reg.list().is_empty());
    }
}
