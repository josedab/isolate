//! Policy intermediate representation.

use serde::{Deserialize, Serialize};

/// A complete policy in IR form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyIR {
    pub name: String,
    pub description: Option<String>,
    pub blocks: Vec<PolicyBlock>,
}

impl PolicyIR {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), description: None, blocks: Vec::new() }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn add_block(mut self, block: PolicyBlock) -> Self {
        self.blocks.push(block);
        self
    }

    pub fn has_block(&self, kind: &BlockKind) -> bool {
        self.blocks.iter().any(|b| std::mem::discriminant(&b.kind) == std::mem::discriminant(kind))
    }
}

/// A single block in the policy IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBlock {
    pub id: String,
    pub kind: BlockKind,
    pub enabled: bool,
}

impl PolicyBlock {
    pub fn new(id: impl Into<String>, kind: BlockKind) -> Self {
        Self { id: id.into(), kind, enabled: true }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Types of policy blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockKind {
    Resource(ResourceBlock),
    Capability(CapabilityBlock),
    Network(NetworkBlock),
    Environment(EnvironmentBlock),
}

/// Resource limits block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBlock {
    pub max_memory_bytes: Option<u64>,
    pub max_fuel: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub max_io_bytes: Option<u64>,
}

impl Default for ResourceBlock {
    fn default() -> Self {
        Self {
            max_memory_bytes: Some(64 * 1024 * 1024), // 64MB
            max_fuel: Some(1_000_000),
            timeout_ms: Some(30_000),
            max_io_bytes: Some(10 * 1024 * 1024), // 10MB
        }
    }
}

/// Capability grants block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityBlock {
    pub stdout: bool,
    pub stderr: bool,
    pub stdin: bool,
    pub filesystem_read: Vec<String>,
    pub filesystem_write: Vec<String>,
    pub env_vars: Vec<String>,
}

impl Default for CapabilityBlock {
    fn default() -> Self {
        Self {
            stdout: true,
            stderr: true,
            stdin: false,
            filesystem_read: Vec::new(),
            filesystem_write: Vec::new(),
            env_vars: Vec::new(),
        }
    }
}

/// Network access rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkBlock {
    pub allow_outbound: bool,
    pub allowed_hosts: Vec<String>,
    pub allowed_ports: Vec<u16>,
    pub max_connections: Option<u32>,
}

/// Environment variable configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentBlock {
    pub inherit: bool,
    pub variables: Vec<(String, String)>,
    pub passthrough: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_policy_ir() {
        let ir = PolicyIR::new("test-policy")
            .with_description("A test policy")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())))
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock::default())));

        assert_eq!(ir.name, "test-policy");
        assert_eq!(ir.blocks.len(), 2);
        assert!(ir.has_block(&BlockKind::Resource(ResourceBlock::default())));
    }

    #[test]
    fn test_disabled_block() {
        let block = PolicyBlock::new("net", BlockKind::Network(NetworkBlock::default())).disabled();
        assert!(!block.enabled);
    }

    #[test]
    fn test_resource_defaults() {
        let r = ResourceBlock::default();
        assert_eq!(r.max_memory_bytes, Some(64 * 1024 * 1024));
        assert_eq!(r.max_fuel, Some(1_000_000));
    }

    #[test]
    fn test_capability_defaults() {
        let c = CapabilityBlock::default();
        assert!(c.stdout);
        assert!(c.stderr);
        assert!(!c.stdin);
        assert!(c.filesystem_read.is_empty());
    }

    #[test]
    fn test_network_defaults() {
        let n = NetworkBlock::default();
        assert!(!n.allow_outbound);
        assert!(n.allowed_hosts.is_empty());
    }
}
