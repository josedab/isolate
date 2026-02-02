//! Infrastructure as Code (IaC) Provider Support.
//!
//! Resource definitions and state management for Terraform and Pulumi providers.
//!
//! Provides:
//! - Resource type definitions matching IaC provider schemas
//! - State management for tracking resource lifecycle
//! - Plan/apply workflow support
//! - Import existing resources into IaC state
#![allow(dead_code)]

pub mod declarative;
pub mod plan;
pub mod resource;
pub mod state;

pub use declarative::{
    ConfigError, ConfigLoader, EnvironmentOverride, ResourceSpec, SandboxFile, SandboxSpec,
    parse_duration, parse_size,
};
pub use plan::{
    ActionType, ApplyError, ApplyResult, ExecutionPlan, PlanBuilder, PlanSummary, PlannedAction,
};
pub use resource::{
    CapabilityPolicyResource, IacResource, LifecyclePolicy, PolicyAction, PolicyRule, PoolResource,
    PropertySchema, PropertyType, ResourceSchema, ResourceType, SandboxResource, ScalingPolicy,
    ValidationError,
};
pub use state::{PropertyChange, ResourceState, StateDiff, StateEntry, StateStore};
