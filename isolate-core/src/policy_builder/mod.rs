//! Visual Policy Builder.
//!
//! Intermediate representation and code generation for building
//! capability policies through a visual/programmatic interface.
//!
//! # Features
//!
//! - **Policy IR**: Block-based intermediate representation
//! - **Validation**: Real-time policy validation with detailed errors
//! - **Code Generation**: Convert IR to HCL policy language
//! - **Templates**: Pre-built policy templates for common use cases
//! - **Simulation**: Dry-run policies against test scenarios



pub mod codegen;
pub mod ir;
pub mod simulation;
pub mod templates;
pub mod validation;

pub use codegen::PolicyCodegen;
pub use ir::{PolicyBlock, PolicyIR, BlockKind, ResourceBlock, CapabilityBlock};
pub use simulation::{PolicySimulator, SimulationResult, SimulatedAction};
pub use templates::{PolicyTemplate, TemplateLibrary};
pub use validation::{PolicyValidator, ValidationIssue, IssueSeverity};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_to_ir_to_hcl() {
        let lib = TemplateLibrary::default();
        let template = lib.get("web-handler").unwrap();
        let ir = template.to_ir();

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().all(|i| i.severity != IssueSeverity::Error));

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.contains("sandbox"));
        assert!(!hcl.is_empty());
    }
}
