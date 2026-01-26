//! # Policy-as-Code Language
//!
//! A domain-specific language for expressing sandbox policies with an
//! HCL-inspired syntax. Enables GitOps workflows with version-controlled,
//! composable security policies.
//!
//! ## Policy Syntax
//!
//! ```text
//! sandbox "web-handler" {
//!   resource {
//!     memory_limit = "128MB"
//!     fuel = 1000000
//!     timeout = "30s"
//!   }
//!   capability {
//!     allow_stdout = true
//!     allow_stderr = true
//!     fs_read = ["/data", "/config"]
//!   }
//!   network {
//!     allow_dns = true
//!     allow_http = ["api.example.com"]
//!   }
//! }
//! ```

mod eval;
pub mod lint;
mod parser;

pub use eval::{PolicyEvaluator, ResolvedPolicy};
pub use lint::{LintFinding, LintResult, LintSeverity, PolicyLinter, PolicyTest, run_policy_tests};
pub use parser::{
    CapabilityBlock, NetworkBlock, ParseError, PolicyDocument, PolicyParser, ResourceBlock,
    SandboxPolicy,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_evaluate_flow() {
        let input = r#"
            sandbox "api-handler" {
                resource {
                    memory_limit = "256MB"
                    fuel = 2000000
                    timeout = "60s"
                }
                capability {
                    allow_stdout = true
                    allow_stderr = false
                    fs_read = ["/data"]
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();
        assert_eq!(doc.policies.len(), 1);
        assert_eq!(doc.policies[0].name, "api-handler");

        let evaluator = PolicyEvaluator::new();
        let resolved = evaluator.resolve(&doc.policies[0]).unwrap();

        assert_eq!(resolved.memory_limit_bytes, 256 * 1024 * 1024);
        assert_eq!(resolved.fuel, Some(2_000_000));
        assert!(resolved.allow_stdout);
        assert!(!resolved.allow_stderr);
        assert_eq!(resolved.fs_read_paths, vec!["/data".to_string()]);
    }
}
