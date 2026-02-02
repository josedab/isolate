//! State management for IaC-managed resources.

use super::resource::{IacResource, ResourceType};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State of a managed resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceState {
    Planned,
    Creating,
    Created,
    Updating,
    Deleting,
    Deleted,
    Error { message: String },
}

/// State entry for one resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEntry {
    pub resource_type: ResourceType,
    pub name: String,
    pub id: String,
    pub state: ResourceState,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Version counter.
    pub serial: u64,
}

/// State store for all managed resources.
pub struct StateStore {
    entries: HashMap<String, StateEntry>,
    serial: u64,
    lock_holder: Option<String>,
}

/// A diff between desired and current state.
#[derive(Debug, Clone)]
pub enum StateDiff {
    Create { resource: IacResource },
    Update { name: String, changes: Vec<PropertyChange> },
    Delete { name: String, resource_type: ResourceType },
    NoChange { name: String },
}

/// A single property change.
pub struct PropertyChange {
    pub field: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

impl std::fmt::Debug for PropertyChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyChange")
            .field("field", &self.field)
            .field("old_value", &self.old_value)
            .field("new_value", &self.new_value)
            .finish()
    }
}

impl Clone for PropertyChange {
    fn clone(&self) -> Self {
        PropertyChange {
            field: self.field.clone(),
            old_value: self.old_value.clone(),
            new_value: self.new_value.clone(),
        }
    }
}

fn state_key(resource_type: &ResourceType, name: &str) -> String {
    format!("{}.{}", resource_type, name)
}

impl StateStore {
    pub fn new() -> Self {
        StateStore { entries: HashMap::new(), serial: 0, lock_holder: None }
    }

    pub fn add(&mut self, entry: StateEntry) {
        let key = state_key(&entry.resource_type, &entry.name);
        self.serial += 1;
        self.entries.insert(key, entry);
    }

    pub fn remove(&mut self, key: &str) -> Option<StateEntry> {
        self.serial += 1;
        self.entries.remove(key)
    }

    pub fn get(&self, key: &str) -> Option<&StateEntry> {
        self.entries.get(key)
    }

    pub fn list(&self) -> Vec<&StateEntry> {
        self.entries.values().collect()
    }

    pub fn lock(&mut self, holder: &str) -> Result<()> {
        if let Some(ref current) = self.lock_holder {
            return Err(Error::InvalidConfig(format!("State is already locked by '{}'", current)));
        }
        self.lock_holder = Some(holder.to_string());
        Ok(())
    }

    pub fn unlock(&mut self, holder: &str) -> Result<()> {
        match &self.lock_holder {
            Some(current) if current == holder => {
                self.lock_holder = None;
                Ok(())
            }
            Some(current) => Err(Error::InvalidConfig(format!(
                "Cannot unlock: held by '{}', not '{}'",
                current, holder
            ))),
            None => Err(Error::InvalidConfig("State is not locked".into())),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.lock_holder.is_some()
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn export_json(&self) -> String {
        let entries: Vec<&StateEntry> = self.entries.values().collect();
        serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn import_json(&mut self, json: &str) -> Result<()> {
        let entries: Vec<StateEntry> = serde_json::from_str(json)
            .map_err(|e| Error::InvalidConfig(format!("Invalid state JSON: {}", e)))?;
        for entry in entries {
            let key = state_key(&entry.resource_type, &entry.name);
            self.entries.insert(key, entry);
        }
        self.serial += 1;
        Ok(())
    }

    /// Computes the diff between the desired resources and current state.
    pub fn diff(&self, desired: &[IacResource]) -> Vec<StateDiff> {
        let mut diffs = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();

        for resource in desired {
            let key = state_key(&resource.resource_type, &resource.name);
            seen_keys.insert(key.clone());

            match self.entries.get(&key) {
                None => {
                    diffs.push(StateDiff::Create { resource: resource.clone() });
                }
                Some(existing) => {
                    let changes =
                        compute_property_changes(&existing.properties, &resource.properties);
                    if changes.is_empty() {
                        diffs.push(StateDiff::NoChange { name: resource.name.clone() });
                    } else {
                        diffs.push(StateDiff::Update { name: resource.name.clone(), changes });
                    }
                }
            }
        }

        // Resources in state but not in desired → delete
        for (key, entry) in &self.entries {
            if !seen_keys.contains(key) {
                diffs.push(StateDiff::Delete {
                    name: entry.name.clone(),
                    resource_type: entry.resource_type.clone(),
                });
            }
        }

        diffs
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}

fn compute_property_changes(
    old: &HashMap<String, serde_json::Value>,
    new: &HashMap<String, serde_json::Value>,
) -> Vec<PropertyChange> {
    let mut changes = Vec::new();

    for (key, new_val) in new {
        match old.get(key) {
            Some(old_val) if old_val != new_val => {
                changes.push(PropertyChange {
                    field: key.clone(),
                    old_value: old_val.clone(),
                    new_value: new_val.clone(),
                });
            }
            None => {
                changes.push(PropertyChange {
                    field: key.clone(),
                    old_value: serde_json::Value::Null,
                    new_value: new_val.clone(),
                });
            }
            _ => {}
        }
    }

    for key in old.keys() {
        if !new.contains_key(key) {
            changes.push(PropertyChange {
                field: key.clone(),
                old_value: old[key].clone(),
                new_value: serde_json::Value::Null,
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iac::resource::LifecyclePolicy;

    fn make_entry(name: &str, resource_type: ResourceType) -> StateEntry {
        StateEntry {
            resource_type,
            name: name.into(),
            id: format!("id-{}", name),
            state: ResourceState::Created,
            properties: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            serial: 1,
        }
    }

    fn make_iac_resource(name: &str, resource_type: ResourceType) -> IacResource {
        IacResource {
            resource_type,
            name: name.into(),
            id: None,
            properties: HashMap::new(),
            labels: HashMap::new(),
            depends_on: Vec::new(),
            lifecycle: LifecyclePolicy::default(),
        }
    }

    #[test]
    fn test_state_store_add_and_get() {
        let mut store = StateStore::new();
        let entry = make_entry("sandbox-1", ResourceType::Sandbox);
        store.add(entry);

        let result = store.get("sandbox.sandbox-1");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "sandbox-1");
    }

    #[test]
    fn test_state_store_remove() {
        let mut store = StateStore::new();
        store.add(make_entry("sandbox-1", ResourceType::Sandbox));

        let removed = store.remove("sandbox.sandbox-1");
        assert!(removed.is_some());
        assert!(store.get("sandbox.sandbox-1").is_none());
    }

    #[test]
    fn test_state_store_list() {
        let mut store = StateStore::new();
        store.add(make_entry("a", ResourceType::Sandbox));
        store.add(make_entry("b", ResourceType::Module));

        let entries = store.list();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_state_store_locking() {
        let mut store = StateStore::new();
        assert!(!store.is_locked());

        store.lock("user-1").unwrap();
        assert!(store.is_locked());

        // Double lock should fail
        assert!(store.lock("user-2").is_err());

        // Wrong holder unlock should fail
        assert!(store.unlock("user-2").is_err());

        // Correct holder unlock should succeed
        store.unlock("user-1").unwrap();
        assert!(!store.is_locked());
    }

    #[test]
    fn test_state_store_unlock_when_not_locked() {
        let mut store = StateStore::new();
        assert!(store.unlock("anyone").is_err());
    }

    #[test]
    fn test_state_store_export_import_json() {
        let mut store = StateStore::new();
        let mut entry = make_entry("sandbox-1", ResourceType::Sandbox);
        entry.properties.insert("memory".into(), serde_json::json!(128));
        store.add(entry);

        let json = store.export_json();
        assert!(!json.is_empty());

        let mut new_store = StateStore::new();
        new_store.import_json(&json).unwrap();

        let entries = new_store.list();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "sandbox-1");
    }

    #[test]
    fn test_state_store_import_invalid_json() {
        let mut store = StateStore::new();
        let result = store.import_json("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_diff_create() {
        let store = StateStore::new();
        let desired = vec![make_iac_resource("new-sandbox", ResourceType::Sandbox)];

        let diffs = store.diff(&desired);
        assert_eq!(diffs.len(), 1);
        assert!(
            matches!(&diffs[0], StateDiff::Create { resource } if resource.name == "new-sandbox")
        );
    }

    #[test]
    fn test_diff_delete() {
        let mut store = StateStore::new();
        store.add(make_entry("old-sandbox", ResourceType::Sandbox));

        let diffs = store.diff(&[]);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(&diffs[0], StateDiff::Delete { name, .. } if name == "old-sandbox"));
    }

    #[test]
    fn test_diff_no_change() {
        let mut store = StateStore::new();
        store.add(make_entry("sandbox-1", ResourceType::Sandbox));

        let desired = vec![make_iac_resource("sandbox-1", ResourceType::Sandbox)];
        let diffs = store.diff(&desired);

        assert_eq!(diffs.len(), 1);
        assert!(matches!(&diffs[0], StateDiff::NoChange { name } if name == "sandbox-1"));
    }

    #[test]
    fn test_diff_update() {
        let mut store = StateStore::new();
        let mut entry = make_entry("sandbox-1", ResourceType::Sandbox);
        entry.properties.insert("memory".into(), serde_json::json!(128));
        store.add(entry);

        let mut resource = make_iac_resource("sandbox-1", ResourceType::Sandbox);
        resource.properties.insert("memory".into(), serde_json::json!(256));

        let diffs = store.diff(&[resource]);
        assert_eq!(diffs.len(), 1);
        assert!(
            matches!(&diffs[0], StateDiff::Update { name, changes } if name == "sandbox-1" && !changes.is_empty())
        );
    }

    #[test]
    fn test_resource_state_serialization() {
        let states = vec![
            ResourceState::Planned,
            ResourceState::Creating,
            ResourceState::Created,
            ResourceState::Updating,
            ResourceState::Deleting,
            ResourceState::Deleted,
            ResourceState::Error { message: "something failed".into() },
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: ResourceState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }

    #[test]
    fn test_serial_increments() {
        let mut store = StateStore::new();
        assert_eq!(store.serial(), 0);

        store.add(make_entry("a", ResourceType::Sandbox));
        assert_eq!(store.serial(), 1);

        store.add(make_entry("b", ResourceType::Sandbox));
        assert_eq!(store.serial(), 2);

        store.remove("sandbox.a");
        assert_eq!(store.serial(), 3);
    }
}
