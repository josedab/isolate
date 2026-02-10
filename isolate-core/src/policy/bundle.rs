//! Policy bundle management with hot-reload support.
//!
//! Enables loading, versioning, and hot-swapping of policy bundles at runtime
//! without restarting the sandbox runtime.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::policy::bundle::{PolicyBundle, BundleManager};
//!
//! let mut manager = BundleManager::new();
//!
//! // Load a bundle
//! let bundle = PolicyBundle::new("security-baseline", "1.0.0")
//!     .with_rule(allow_stdout_rule)
//!     .with_rule(deny_network_rule);
//!
//! manager.load(bundle)?;
//!
//! // Hot-reload with a new version
//! let updated = PolicyBundle::new("security-baseline", "1.1.0")
//!     .with_rule(allow_stdout_rule)
//!     .with_rule(allow_dns_rule);
//!
//! manager.reload("security-baseline", updated)?;
//! ```

use super::engine::PolicyEngine;
use super::rules::{Effect, PolicyRule};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A versioned collection of policy rules that can be loaded/unloaded atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBundle {
    /// Bundle name (unique identifier).
    pub name: String,
    /// Bundle version (semver-style).
    pub version: String,
    /// Description.
    pub description: Option<String>,
    /// Policy rules in this bundle.
    pub rules: Vec<PolicyRule>,
    /// Bundle metadata.
    pub metadata: HashMap<String, String>,
    /// Whether this bundle is enabled.
    pub enabled: bool,
    /// Priority for conflict resolution between bundles (higher wins).
    pub priority: u32,
}

impl PolicyBundle {
    /// Create a new policy bundle.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            rules: Vec::new(),
            metadata: HashMap::new(),
            enabled: true,
            priority: 100,
        }
    }

    /// Add a rule to the bundle.
    pub fn with_rule(mut self, rule: PolicyRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get the number of rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// Error from bundle operations.
#[derive(Debug, Clone)]
pub enum BundleError {
    /// Bundle not found.
    NotFound(String),
    /// Bundle with this name already exists.
    AlreadyExists(String),
    /// Validation failed.
    ValidationFailed(Vec<String>),
    /// Version conflict.
    VersionConflict { name: String, existing: String, new: String },
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "bundle '{}' not found", name),
            Self::AlreadyExists(name) => write!(f, "bundle '{}' already exists", name),
            Self::ValidationFailed(errs) => write!(f, "validation failed: {}", errs.join(", ")),
            Self::VersionConflict { name, existing, new } => {
                write!(f, "bundle '{}' version conflict: {} vs {}", name, existing, new)
            }
        }
    }
}

impl std::error::Error for BundleError {}

/// Record of a bundle load/reload event.
#[derive(Debug, Clone)]
pub struct BundleEvent {
    /// Bundle name.
    pub bundle_name: String,
    /// Event type.
    pub event_type: BundleEventType,
    /// Version.
    pub version: String,
    /// When the event occurred.
    pub timestamp: std::time::SystemTime,
}

/// Type of bundle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleEventType {
    Loaded,
    Reloaded,
    Unloaded,
    Enabled,
    Disabled,
}

/// Manages policy bundles with hot-reload support.
pub struct BundleManager {
    bundles: HashMap<String, PolicyBundle>,
    engine: PolicyEngine,
    history: Vec<BundleEvent>,
    reload_count: u64,
}

impl Default for BundleManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BundleManager {
    /// Create a new bundle manager.
    pub fn new() -> Self {
        Self {
            bundles: HashMap::new(),
            engine: PolicyEngine::new(),
            history: Vec::new(),
            reload_count: 0,
        }
    }

    /// Load a new policy bundle.
    pub fn load(&mut self, bundle: PolicyBundle) -> Result<(), BundleError> {
        if self.bundles.contains_key(&bundle.name) {
            return Err(BundleError::AlreadyExists(bundle.name.clone()));
        }

        self.validate_bundle(&bundle)?;

        let name = bundle.name.clone();
        let version = bundle.version.clone();

        // Add rules to engine
        if bundle.enabled {
            for rule in &bundle.rules {
                self.engine.add_rule(rule.clone());
            }
        }

        self.bundles.insert(name.clone(), bundle);
        self.record_event(&name, BundleEventType::Loaded, &version);

        Ok(())
    }

    /// Hot-reload a bundle with a new version.
    ///
    /// Atomically removes all old rules and adds the new ones.
    pub fn reload(
        &mut self,
        bundle_name: &str,
        new_bundle: PolicyBundle,
    ) -> Result<(), BundleError> {
        let old = self
            .bundles
            .get(bundle_name)
            .ok_or_else(|| BundleError::NotFound(bundle_name.to_string()))?;

        // Validate new bundle
        self.validate_bundle(&new_bundle)?;

        let _old_version = old.version.clone();
        let new_version = new_bundle.version.clone();

        // Remove old rules from engine
        self.rebuild_engine_without(bundle_name);

        // Add new rules
        if new_bundle.enabled {
            for rule in &new_bundle.rules {
                self.engine.add_rule(rule.clone());
            }
        }

        self.bundles.insert(bundle_name.to_string(), new_bundle);
        self.reload_count += 1;
        self.record_event(bundle_name, BundleEventType::Reloaded, &new_version);

        Ok(())
    }

    /// Unload a bundle, removing all its rules.
    pub fn unload(&mut self, bundle_name: &str) -> Result<(), BundleError> {
        let bundle = self
            .bundles
            .remove(bundle_name)
            .ok_or_else(|| BundleError::NotFound(bundle_name.to_string()))?;

        // Rebuild engine without this bundle
        self.rebuild_engine_without(bundle_name);
        self.record_event(bundle_name, BundleEventType::Unloaded, &bundle.version);

        Ok(())
    }

    /// Enable a disabled bundle.
    pub fn enable(&mut self, bundle_name: &str) -> Result<(), BundleError> {
        let bundle = self
            .bundles
            .get(bundle_name)
            .ok_or_else(|| BundleError::NotFound(bundle_name.to_string()))?;

        if !bundle.enabled {
            let version = bundle.version.clone();
            let rules: Vec<_> = bundle.rules.clone();

            let bundle_mut = self.bundles.get_mut(bundle_name).unwrap();
            bundle_mut.enabled = true;

            for rule in &rules {
                self.engine.add_rule(rule.clone());
            }
            self.record_event(bundle_name, BundleEventType::Enabled, &version);
        }

        Ok(())
    }

    /// Disable a bundle (rules remain stored but aren't evaluated).
    pub fn disable(&mut self, bundle_name: &str) -> Result<(), BundleError> {
        let bundle = self
            .bundles
            .get(bundle_name)
            .ok_or_else(|| BundleError::NotFound(bundle_name.to_string()))?;

        if !bundle.enabled {
            return Ok(());
        }

        let version = bundle.version.clone();

        let bundle_mut = self.bundles.get_mut(bundle_name).unwrap();
        bundle_mut.enabled = false;

        self.rebuild_engine_without(bundle_name);
        self.record_event(bundle_name, BundleEventType::Disabled, &version);

        Ok(())
    }

    /// Get a reference to a loaded bundle.
    pub fn get(&self, bundle_name: &str) -> Option<&PolicyBundle> {
        self.bundles.get(bundle_name)
    }

    /// List all loaded bundles.
    pub fn list(&self) -> Vec<&PolicyBundle> {
        let mut bundles: Vec<_> = self.bundles.values().collect();
        bundles.sort_by(|a, b| b.priority.cmp(&a.priority));
        bundles
    }

    /// Get the underlying policy engine for evaluation.
    pub fn engine(&self) -> &PolicyEngine {
        &self.engine
    }

    /// Get the total number of loaded bundles.
    pub fn bundle_count(&self) -> usize {
        self.bundles.len()
    }

    /// Get the total number of active rules across all enabled bundles.
    pub fn active_rule_count(&self) -> usize {
        self.engine.rule_count()
    }

    /// Get the hot-reload count.
    pub fn reload_count(&self) -> u64 {
        self.reload_count
    }

    /// Get the event history.
    pub fn history(&self) -> &[BundleEvent] {
        &self.history
    }

    /// Dry-run evaluate: check what a bundle would do for a given action/resource/principal.
    pub fn dry_run(
        &self,
        bundle: &PolicyBundle,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, super::rules::Value>,
    ) -> Vec<DryRunResult> {
        let mut results = Vec::new();

        for rule in &bundle.rules {
            let matches = rule.matches(action, resource, principal, context);
            results.push(DryRunResult {
                rule_id: rule.id.clone(),
                would_match: matches,
                effect: rule.effect,
            });
        }

        results
    }

    fn validate_bundle(&self, bundle: &PolicyBundle) -> Result<(), BundleError> {
        let mut errors = Vec::new();

        if bundle.name.is_empty() {
            errors.push("bundle name cannot be empty".to_string());
        }

        if bundle.version.is_empty() {
            errors.push("bundle version cannot be empty".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(BundleError::ValidationFailed(errors))
        }
    }

    fn rebuild_engine_without(&mut self, exclude_bundle: &str) {
        self.engine = PolicyEngine::new();
        for (name, bundle) in &self.bundles {
            if name != exclude_bundle && bundle.enabled {
                for rule in &bundle.rules {
                    self.engine.add_rule(rule.clone());
                }
            }
        }
    }

    fn record_event(&mut self, bundle_name: &str, event_type: BundleEventType, version: &str) {
        self.history.push(BundleEvent {
            bundle_name: bundle_name.to_string(),
            event_type,
            version: version.to_string(),
            timestamp: std::time::SystemTime::now(),
        });
    }
}

/// Result of a dry-run evaluation for a single rule.
#[derive(Debug, Clone)]
pub struct DryRunResult {
    /// Rule ID.
    pub rule_id: String,
    /// Whether the rule would match.
    pub would_match: bool,
    /// The effect that would be applied.
    pub effect: Effect,
}

#[cfg(test)]
mod tests {
    use super::super::rules::{Effect, PolicyRule};
    use super::*;

    fn make_allow_rule(name: &str, action: &str) -> PolicyRule {
        PolicyRule::builder(name).effect(Effect::Allow).action(action).build()
    }

    fn make_deny_rule(name: &str, action: &str) -> PolicyRule {
        PolicyRule::builder(name).effect(Effect::Deny).action(action).build()
    }

    #[test]
    fn test_bundle_creation() {
        let bundle = PolicyBundle::new("test", "1.0.0")
            .with_description("Test bundle")
            .with_rule(make_allow_rule("r1", "stdio:*"))
            .with_priority(200);

        assert_eq!(bundle.name, "test");
        assert_eq!(bundle.version, "1.0.0");
        assert_eq!(bundle.rule_count(), 1);
        assert_eq!(bundle.priority, 200);
    }

    #[test]
    fn test_load_bundle() {
        let mut manager = BundleManager::new();
        let bundle =
            PolicyBundle::new("baseline", "1.0.0").with_rule(make_allow_rule("r1", "stdio:*"));

        manager.load(bundle).unwrap();
        assert_eq!(manager.bundle_count(), 1);
        assert_eq!(manager.active_rule_count(), 1);
    }

    #[test]
    fn test_load_duplicate() {
        let mut manager = BundleManager::new();
        manager.load(PolicyBundle::new("baseline", "1.0.0")).unwrap();

        let result = manager.load(PolicyBundle::new("baseline", "1.1.0"));
        assert!(result.is_err());
    }

    #[test]
    fn test_hot_reload() {
        let mut manager = BundleManager::new();
        let v1 = PolicyBundle::new("baseline", "1.0.0").with_rule(make_allow_rule("r1", "stdio:*"));
        manager.load(v1).unwrap();
        assert_eq!(manager.active_rule_count(), 1);

        let v2 = PolicyBundle::new("baseline", "2.0.0")
            .with_rule(make_allow_rule("r1", "stdio:*"))
            .with_rule(make_deny_rule("r2", "net:*"));
        manager.reload("baseline", v2).unwrap();

        assert_eq!(manager.bundle_count(), 1);
        assert_eq!(manager.active_rule_count(), 2);
        assert_eq!(manager.reload_count(), 1);

        let bundle = manager.get("baseline").unwrap();
        assert_eq!(bundle.version, "2.0.0");
    }

    #[test]
    fn test_unload() {
        let mut manager = BundleManager::new();
        manager
            .load(
                PolicyBundle::new("baseline", "1.0.0").with_rule(make_allow_rule("r1", "stdio:*")),
            )
            .unwrap();

        manager.unload("baseline").unwrap();
        assert_eq!(manager.bundle_count(), 0);
        assert_eq!(manager.active_rule_count(), 0);
    }

    #[test]
    fn test_enable_disable() {
        let mut manager = BundleManager::new();
        manager
            .load(
                PolicyBundle::new("baseline", "1.0.0").with_rule(make_allow_rule("r1", "stdio:*")),
            )
            .unwrap();

        assert_eq!(manager.active_rule_count(), 1);

        manager.disable("baseline").unwrap();
        assert_eq!(manager.active_rule_count(), 0);
        assert!(!manager.get("baseline").unwrap().enabled);

        manager.enable("baseline").unwrap();
        assert_eq!(manager.active_rule_count(), 1);
    }

    #[test]
    fn test_multiple_bundles() {
        let mut manager = BundleManager::new();
        manager
            .load(
                PolicyBundle::new("baseline", "1.0.0").with_rule(make_allow_rule("r1", "stdio:*")),
            )
            .unwrap();
        manager
            .load(PolicyBundle::new("network", "1.0.0").with_rule(make_deny_rule("r2", "net:*")))
            .unwrap();

        assert_eq!(manager.bundle_count(), 2);
        assert_eq!(manager.active_rule_count(), 2);

        manager.unload("network").unwrap();
        assert_eq!(manager.bundle_count(), 1);
        assert_eq!(manager.active_rule_count(), 1);
    }

    #[test]
    fn test_list_sorted_by_priority() {
        let mut manager = BundleManager::new();
        manager.load(PolicyBundle::new("low", "1.0.0").with_priority(10)).unwrap();
        manager.load(PolicyBundle::new("high", "1.0.0").with_priority(200)).unwrap();

        let list = manager.list();
        assert_eq!(list[0].name, "high");
        assert_eq!(list[1].name, "low");
    }

    #[test]
    fn test_event_history() {
        let mut manager = BundleManager::new();
        manager.load(PolicyBundle::new("baseline", "1.0.0")).unwrap();
        manager.reload("baseline", PolicyBundle::new("baseline", "2.0.0")).unwrap();

        let history = manager.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].event_type, BundleEventType::Loaded);
        assert_eq!(history[1].event_type, BundleEventType::Reloaded);
    }

    #[test]
    fn test_validation_error() {
        let mut manager = BundleManager::new();
        let result = manager.load(PolicyBundle::new("", "1.0.0"));
        assert!(result.is_err());
    }

    #[test]
    fn test_not_found_error() {
        let mut manager = BundleManager::new();
        let result = manager.unload("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_bundle_error_display() {
        let err = BundleError::NotFound("test".to_string());
        assert_eq!(err.to_string(), "bundle 'test' not found");
    }
}
