//! Plan/apply workflow for IaC operations.

use super::resource::{IacResource, ResourceType};
use super::state::{StateDiff, StateStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Execution plan for IaC changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub id: String,
    pub actions: Vec<PlannedAction>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub summary: PlanSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    pub action_type: ActionType,
    pub resource_type: ResourceType,
    pub resource_name: String,
    pub details: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    Create,
    Update,
    Delete,
    Replace,
    NoOp,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanSummary {
    pub to_create: usize,
    pub to_update: usize,
    pub to_delete: usize,
    pub to_replace: usize,
    pub unchanged: usize,
}

impl std::fmt::Display for PlanSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Plan: {} to create, {} to update, {} to delete, {} to replace, {} unchanged",
            self.to_create, self.to_update, self.to_delete, self.to_replace, self.unchanged
        )
    }
}

/// Plan builder that computes required changes.
pub struct PlanBuilder {
    desired: Vec<IacResource>,
    state: StateStore,
}

impl PlanBuilder {
    pub fn new(state: StateStore) -> Self {
        PlanBuilder { desired: Vec::new(), state }
    }

    pub fn add_resource(&mut self, resource: IacResource) {
        self.desired.push(resource);
    }

    /// Builds an execution plan from the desired state and current state.
    pub fn build(self) -> ExecutionPlan {
        let diffs = self.state.diff(&self.desired);
        let mut actions = Vec::new();
        let mut summary = PlanSummary::default();

        for diff in diffs {
            match diff {
                StateDiff::Create { resource } => {
                    summary.to_create += 1;
                    actions.push(PlannedAction {
                        action_type: ActionType::Create,
                        resource_type: resource.resource_type.clone(),
                        resource_name: resource.name.clone(),
                        details: resource.properties.clone(),
                    });
                }
                StateDiff::Update { name, changes } => {
                    summary.to_update += 1;
                    let mut details = HashMap::new();
                    for change in &changes {
                        details.insert(
                            change.field.clone(),
                            serde_json::json!({
                                "old": change.old_value,
                                "new": change.new_value,
                            }),
                        );
                    }
                    // Find resource type from desired resources
                    let resource_type = self
                        .desired
                        .iter()
                        .find(|r| r.name == name)
                        .map(|r| r.resource_type.clone())
                        .unwrap_or(ResourceType::Sandbox);
                    actions.push(PlannedAction {
                        action_type: ActionType::Update,
                        resource_type,
                        resource_name: name,
                        details,
                    });
                }
                StateDiff::Delete { name, resource_type } => {
                    summary.to_delete += 1;
                    actions.push(PlannedAction {
                        action_type: ActionType::Delete,
                        resource_type,
                        resource_name: name,
                        details: HashMap::new(),
                    });
                }
                StateDiff::NoChange { name } => {
                    summary.unchanged += 1;
                    // Find resource type from desired resources
                    let resource_type = self
                        .desired
                        .iter()
                        .find(|r| r.name == name)
                        .map(|r| r.resource_type.clone())
                        .unwrap_or(ResourceType::Sandbox);
                    actions.push(PlannedAction {
                        action_type: ActionType::NoOp,
                        resource_type,
                        resource_name: name,
                        details: HashMap::new(),
                    });
                }
            }
        }

        ExecutionPlan {
            id: uuid::Uuid::new_v4().to_string(),
            actions,
            created_at: chrono::Utc::now(),
            summary,
        }
    }

    /// Preview the plan summary without building the full plan.
    pub fn preview(&self) -> PlanSummary {
        let diffs = self.state.diff(&self.desired);
        let mut summary = PlanSummary::default();
        for diff in diffs {
            match diff {
                StateDiff::Create { .. } => summary.to_create += 1,
                StateDiff::Update { .. } => summary.to_update += 1,
                StateDiff::Delete { .. } => summary.to_delete += 1,
                StateDiff::NoChange { .. } => summary.unchanged += 1,
            }
        }
        summary
    }

    /// Format the plan as a human-readable string.
    pub fn format_plan(&self) -> String {
        let diffs = self.state.diff(&self.desired);
        let mut lines = Vec::new();
        lines.push("Execution Plan:".to_string());
        lines.push(String::new());

        for diff in &diffs {
            match diff {
                StateDiff::Create { resource } => {
                    lines.push(format!(
                        "  + {} \"{}\" ({})",
                        resource.resource_type, resource.name, "create"
                    ));
                }
                StateDiff::Update { name, changes } => {
                    lines.push(format!("  ~ \"{}\" (update)", name));
                    for change in changes {
                        lines.push(format!(
                            "      {} : {} -> {}",
                            change.field, change.old_value, change.new_value
                        ));
                    }
                }
                StateDiff::Delete { name, resource_type } => {
                    lines.push(format!("  - {} \"{}\" (destroy)", resource_type, name));
                }
                StateDiff::NoChange { name } => {
                    lines.push(format!("    \"{}\" (no change)", name));
                }
            }
        }

        let summary = self.preview();
        lines.push(String::new());
        lines.push(summary.to_string());

        lines.join("\n")
    }
}

/// Result of applying a plan.
pub struct ApplyResult {
    pub plan_id: String,
    pub succeeded: Vec<String>,
    pub failed: Vec<ApplyError>,
    pub duration: Duration,
}

/// Error during plan application.
pub struct ApplyError {
    pub resource_name: String,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iac::resource::LifecyclePolicy;
    use crate::iac::state::{ResourceState, StateEntry};

    fn make_resource(name: &str, rt: ResourceType) -> IacResource {
        IacResource {
            resource_type: rt,
            name: name.into(),
            id: None,
            properties: HashMap::new(),
            labels: HashMap::new(),
            depends_on: Vec::new(),
            lifecycle: LifecyclePolicy::default(),
        }
    }

    fn make_entry(name: &str, rt: ResourceType) -> StateEntry {
        StateEntry {
            resource_type: rt,
            name: name.into(),
            id: format!("id-{}", name),
            state: ResourceState::Created,
            properties: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            serial: 1,
        }
    }

    #[test]
    fn test_plan_builder_empty() {
        let store = StateStore::new();
        let builder = PlanBuilder::new(store);
        let plan = builder.build();
        assert!(plan.actions.is_empty());
        assert_eq!(plan.summary.to_create, 0);
    }

    #[test]
    fn test_plan_builder_create() {
        let store = StateStore::new();
        let mut builder = PlanBuilder::new(store);
        builder.add_resource(make_resource("sandbox-1", ResourceType::Sandbox));

        let plan = builder.build();
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type, ActionType::Create);
        assert_eq!(plan.summary.to_create, 1);
    }

    #[test]
    fn test_plan_builder_delete() {
        let mut store = StateStore::new();
        store.add(make_entry("sandbox-1", ResourceType::Sandbox));

        let builder = PlanBuilder::new(store);
        let plan = builder.build();

        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type, ActionType::Delete);
        assert_eq!(plan.summary.to_delete, 1);
    }

    #[test]
    fn test_plan_builder_update() {
        let mut store = StateStore::new();
        let mut entry = make_entry("sandbox-1", ResourceType::Sandbox);
        entry.properties.insert("memory".into(), serde_json::json!(128));
        store.add(entry);

        let mut builder = PlanBuilder::new(store);
        let mut resource = make_resource("sandbox-1", ResourceType::Sandbox);
        resource.properties.insert("memory".into(), serde_json::json!(256));
        builder.add_resource(resource);

        let plan = builder.build();
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type, ActionType::Update);
        assert_eq!(plan.summary.to_update, 1);
    }

    #[test]
    fn test_plan_builder_no_change() {
        let mut store = StateStore::new();
        store.add(make_entry("sandbox-1", ResourceType::Sandbox));

        let mut builder = PlanBuilder::new(store);
        builder.add_resource(make_resource("sandbox-1", ResourceType::Sandbox));

        let plan = builder.build();
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type, ActionType::NoOp);
        assert_eq!(plan.summary.unchanged, 1);
    }

    #[test]
    fn test_plan_builder_preview() {
        let store = StateStore::new();
        let mut builder = PlanBuilder::new(store);
        builder.add_resource(make_resource("a", ResourceType::Sandbox));
        builder.add_resource(make_resource("b", ResourceType::Module));

        let summary = builder.preview();
        assert_eq!(summary.to_create, 2);
        assert_eq!(summary.to_delete, 0);
    }

    #[test]
    fn test_plan_builder_format_plan() {
        let store = StateStore::new();
        let mut builder = PlanBuilder::new(store);
        builder.add_resource(make_resource("sandbox-1", ResourceType::Sandbox));

        let output = builder.format_plan();
        assert!(output.contains("Execution Plan:"));
        assert!(output.contains("sandbox-1"));
        assert!(output.contains("create"));
    }

    #[test]
    fn test_plan_summary_display() {
        let summary =
            PlanSummary { to_create: 2, to_update: 1, to_delete: 0, to_replace: 0, unchanged: 3 };
        let display = summary.to_string();
        assert!(display.contains("2 to create"));
        assert!(display.contains("1 to update"));
        assert!(display.contains("3 unchanged"));
    }

    #[test]
    fn test_action_type_serialization() {
        let types = vec![
            ActionType::Create,
            ActionType::Update,
            ActionType::Delete,
            ActionType::Replace,
            ActionType::NoOp,
        ];
        for at in types {
            let json = serde_json::to_string(&at).unwrap();
            let deserialized: ActionType = serde_json::from_str(&json).unwrap();
            assert_eq!(at, deserialized);
        }
    }

    #[test]
    fn test_execution_plan_serialization() {
        let plan = ExecutionPlan {
            id: "plan-123".into(),
            actions: vec![PlannedAction {
                action_type: ActionType::Create,
                resource_type: ResourceType::Sandbox,
                resource_name: "test".into(),
                details: HashMap::new(),
            }],
            created_at: chrono::Utc::now(),
            summary: PlanSummary { to_create: 1, ..Default::default() },
        };
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: ExecutionPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "plan-123");
        assert_eq!(deserialized.actions.len(), 1);
    }

    #[test]
    fn test_mixed_plan() {
        let mut store = StateStore::new();
        store.add(make_entry("keep", ResourceType::Sandbox));
        store.add(make_entry("remove", ResourceType::Module));

        let mut builder = PlanBuilder::new(store);
        builder.add_resource(make_resource("keep", ResourceType::Sandbox));
        builder.add_resource(make_resource("new-one", ResourceType::Sandbox));

        let plan = builder.build();
        assert_eq!(plan.summary.unchanged, 1);
        assert_eq!(plan.summary.to_create, 1);
        assert_eq!(plan.summary.to_delete, 1);
        assert_eq!(plan.actions.len(), 3);
    }
}
