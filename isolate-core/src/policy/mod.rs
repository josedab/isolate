//! Declarative Policy Engine.
//!
//! Cedar-style policy language for defining sandbox security policies
//! declaratively. Supports RBAC, ABAC, and context-aware capability grants.
//!
//! # Features
//!
//! - **Declarative Rules**: Define policies as data, not code
//! - **RBAC/ABAC Support**: Role-based and attribute-based access control
//! - **Context-Aware**: Evaluate policies based on runtime context
//! - **Composable**: Combine multiple policies with conflict resolution
//! - **Auditable**: Full evaluation trace for compliance
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::policy::{PolicyEngine, PolicySet, PolicyRule, Effect};
//!
//! let mut engine = PolicyEngine::new();
//!
//! // Allow stdout for all sandboxes
//! engine.add_rule(PolicyRule::new("allow-stdout")
//!     .effect(Effect::Allow)
//!     .action("stdio:stdout")
//!     .build());
//!
//! // Deny network for untrusted tenants
//! engine.add_rule(PolicyRule::new("deny-untrusted-network")
//!     .effect(Effect::Deny)
//!     .action("net:*")
//!     .condition("tenant.trust_level", Operator::Lt, Value::Int(3))
//!     .build());
//!
//! let decision = engine.evaluate(&context)?;
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.


pub mod bundle;
pub mod dashboard;
mod engine;
pub mod evaluator;
pub mod governance;
mod rules;
pub mod yaml_parser;

pub use bundle::{
    BundleError, BundleEvent, BundleEventType, BundleManager, DryRunResult, PolicyBundle,
};
pub use dashboard::{
    DashboardConfig, DashboardSnapshot, DashboardStats, EvalRecord, PolicyDashboard, RecordFilter,
    RuleStats, WhatIfResult,
};
pub use engine::{EvalTrace, EvalTraceEntry, PolicyDecision, PolicyEngine};
pub use rules::{Condition, Effect, Operator, PolicyRule, PolicyRuleBuilder, PolicySet, Value};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let engine = PolicyEngine::new();
        assert_eq!(engine.rule_count(), 0);
    }
}
