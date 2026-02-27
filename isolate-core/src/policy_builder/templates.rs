//! Pre-built policy templates.

use serde::{Deserialize, Serialize};

use super::ir::*;

/// A pre-built policy template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub blocks: Vec<PolicyBlock>,
}

impl PolicyTemplate {
    /// Convert this template into a PolicyIR for editing.
    pub fn to_ir(&self) -> PolicyIR {
        PolicyIR {
            name: self.id.clone(),
            description: Some(self.description.clone()),
            blocks: self.blocks.clone(),
        }
    }
}

/// Library of pre-built policy templates.
pub struct TemplateLibrary {
    templates: Vec<PolicyTemplate>,
}

impl TemplateLibrary {
    pub fn new() -> Self {
        Self {
            templates: vec![
                Self::web_handler(),
                Self::batch_processor(),
                Self::ml_inference(),
                Self::restricted_plugin(),
            ],
        }
    }

    /// Get a template by ID.
    pub fn get(&self, id: &str) -> Option<&PolicyTemplate> {
        self.templates.iter().find(|t| t.id == id)
    }

    /// List all templates.
    pub fn list(&self) -> &[PolicyTemplate] {
        &self.templates
    }

    /// List templates by category.
    pub fn by_category(&self, category: &str) -> Vec<&PolicyTemplate> {
        self.templates.iter().filter(|t| t.category == category).collect()
    }

    fn web_handler() -> PolicyTemplate {
        PolicyTemplate {
            id: "web-handler".into(),
            name: "Web Request Handler".into(),
            description: "Standard web request handler with stdout and network access".into(),
            category: "web".into(),
            blocks: vec![
                PolicyBlock::new(
                    "resource",
                    BlockKind::Resource(ResourceBlock {
                        max_memory_bytes: Some(128 * 1024 * 1024),
                        max_fuel: Some(10_000_000),
                        timeout_ms: Some(30_000),
                        max_io_bytes: Some(50 * 1024 * 1024),
                    }),
                ),
                PolicyBlock::new(
                    "capability",
                    BlockKind::Capability(CapabilityBlock {
                        stdout: true,
                        stderr: true,
                        stdin: true,
                        ..Default::default()
                    }),
                ),
                PolicyBlock::new(
                    "network",
                    BlockKind::Network(NetworkBlock {
                        allow_outbound: true,
                        allowed_ports: vec![80, 443],
                        ..Default::default()
                    }),
                ),
            ],
        }
    }

    fn batch_processor() -> PolicyTemplate {
        PolicyTemplate {
            id: "batch-processor".into(),
            name: "Batch Data Processor".into(),
            description: "Long-running batch processing with filesystem access".into(),
            category: "processing".into(),
            blocks: vec![
                PolicyBlock::new(
                    "resource",
                    BlockKind::Resource(ResourceBlock {
                        max_memory_bytes: Some(256 * 1024 * 1024),
                        max_fuel: Some(100_000_000),
                        timeout_ms: Some(300_000), // 5 minutes
                        max_io_bytes: Some(500 * 1024 * 1024),
                    }),
                ),
                PolicyBlock::new(
                    "capability",
                    BlockKind::Capability(CapabilityBlock {
                        stdout: true,
                        stderr: true,
                        filesystem_read: vec!["/data/input".into()],
                        filesystem_write: vec!["/data/output".into()],
                        ..Default::default()
                    }),
                ),
            ],
        }
    }

    fn ml_inference() -> PolicyTemplate {
        PolicyTemplate {
            id: "ml-inference".into(),
            name: "ML Model Inference".into(),
            description: "GPU-friendly ML inference with model file access".into(),
            category: "ml".into(),
            blocks: vec![
                PolicyBlock::new(
                    "resource",
                    BlockKind::Resource(ResourceBlock {
                        max_memory_bytes: Some(512 * 1024 * 1024),
                        max_fuel: Some(50_000_000),
                        timeout_ms: Some(60_000),
                        max_io_bytes: Some(100 * 1024 * 1024),
                    }),
                ),
                PolicyBlock::new(
                    "capability",
                    BlockKind::Capability(CapabilityBlock {
                        stdout: true,
                        stderr: true,
                        filesystem_read: vec!["/models".into()],
                        ..Default::default()
                    }),
                ),
            ],
        }
    }

    fn restricted_plugin() -> PolicyTemplate {
        PolicyTemplate {
            id: "restricted-plugin".into(),
            name: "Restricted Plugin".into(),
            description: "Minimal sandbox for untrusted third-party plugins".into(),
            category: "plugin".into(),
            blocks: vec![
                PolicyBlock::new(
                    "resource",
                    BlockKind::Resource(ResourceBlock {
                        max_memory_bytes: Some(16 * 1024 * 1024),
                        max_fuel: Some(500_000),
                        timeout_ms: Some(5_000),
                        max_io_bytes: Some(1024 * 1024),
                    }),
                ),
                PolicyBlock::new(
                    "capability",
                    BlockKind::Capability(CapabilityBlock {
                        stdout: true,
                        stderr: true,
                        ..Default::default()
                    }),
                ),
            ],
        }
    }
}

impl Default for TemplateLibrary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_library_has_templates() {
        let lib = TemplateLibrary::default();
        assert_eq!(lib.list().len(), 4);
    }

    #[test]
    fn test_get_web_handler() {
        let lib = TemplateLibrary::default();
        let t = lib.get("web-handler").unwrap();
        assert_eq!(t.name, "Web Request Handler");
        assert!(!t.blocks.is_empty());
    }

    #[test]
    fn test_get_nonexistent() {
        let lib = TemplateLibrary::default();
        assert!(lib.get("nonexistent").is_none());
    }

    #[test]
    fn test_template_to_ir() {
        let lib = TemplateLibrary::default();
        let ir = lib.get("batch-processor").unwrap().to_ir();
        assert_eq!(ir.name, "batch-processor");
        assert!(ir.description.is_some());
        assert_eq!(ir.blocks.len(), 2);
    }

    #[test]
    fn test_by_category() {
        let lib = TemplateLibrary::default();
        let web = lib.by_category("web");
        assert_eq!(web.len(), 1);
        assert_eq!(web[0].id, "web-handler");
    }

    #[test]
    fn test_restricted_plugin_limits() {
        let lib = TemplateLibrary::default();
        let t = lib.get("restricted-plugin").unwrap();
        let ir = t.to_ir();
        let res_block =
            ir.blocks.iter().find(|b| matches!(b.kind, BlockKind::Resource(_))).unwrap();
        if let BlockKind::Resource(r) = &res_block.kind {
            assert!(r.max_memory_bytes.unwrap() <= 16 * 1024 * 1024);
            assert!(r.max_fuel.unwrap() <= 500_000);
        }
    }
}
