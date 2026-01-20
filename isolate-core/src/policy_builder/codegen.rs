//! HCL code generation from policy IR.

use super::ir::{BlockKind, CapabilityBlock, NetworkBlock, PolicyIR, ResourceBlock, EnvironmentBlock};

/// Generates HCL policy code from a policy IR.
pub struct PolicyCodegen;

impl PolicyCodegen {
    pub fn new() -> Self {
        Self
    }

    /// Generate HCL source from a policy IR.
    pub fn generate(&self, ir: &PolicyIR) -> String {
        let mut output = String::new();

        if let Some(desc) = &ir.description {
            output.push_str(&format!("# {}\n", desc));
        }

        output.push_str(&format!("sandbox \"{}\" {{\n", ir.name));

        for block in &ir.blocks {
            if !block.enabled {
                output.push_str(&format!("  # Block '{}' is disabled\n", block.id));
                continue;
            }
            match &block.kind {
                BlockKind::Resource(r) => self.gen_resource(r, &mut output),
                BlockKind::Capability(c) => self.gen_capability(c, &mut output),
                BlockKind::Network(n) => self.gen_network(n, &mut output),
                BlockKind::Environment(e) => self.gen_environment(e, &mut output),
            }
        }

        output.push_str("}\n");
        output
    }

    fn gen_resource(&self, r: &ResourceBlock, out: &mut String) {
        out.push_str("  resource {\n");
        if let Some(mem) = r.max_memory_bytes {
            out.push_str(&format!("    max_memory = {}\n", Self::format_bytes(mem)));
        }
        if let Some(fuel) = r.max_fuel {
            out.push_str(&format!("    max_fuel = {}\n", fuel));
        }
        if let Some(timeout) = r.timeout_ms {
            out.push_str(&format!("    timeout = \"{}ms\"\n", timeout));
        }
        if let Some(io) = r.max_io_bytes {
            out.push_str(&format!("    max_io = {}\n", Self::format_bytes(io)));
        }
        out.push_str("  }\n\n");
    }

    fn gen_capability(&self, c: &CapabilityBlock, out: &mut String) {
        out.push_str("  capability {\n");
        if c.stdout { out.push_str("    stdout = true\n"); }
        if c.stderr { out.push_str("    stderr = true\n"); }
        if c.stdin { out.push_str("    stdin = true\n"); }
        for path in &c.filesystem_read {
            out.push_str(&format!("    fs_read = \"{}\"\n", path));
        }
        for path in &c.filesystem_write {
            out.push_str(&format!("    fs_write = \"{}\"\n", path));
        }
        for var in &c.env_vars {
            out.push_str(&format!("    env = \"{}\"\n", var));
        }
        out.push_str("  }\n\n");
    }

    fn gen_network(&self, n: &NetworkBlock, out: &mut String) {
        out.push_str("  network {\n");
        out.push_str(&format!("    allow_outbound = {}\n", n.allow_outbound));
        for host in &n.allowed_hosts {
            out.push_str(&format!("    allow_host = \"{}\"\n", host));
        }
        for port in &n.allowed_ports {
            out.push_str(&format!("    allow_port = {}\n", port));
        }
        if let Some(max) = n.max_connections {
            out.push_str(&format!("    max_connections = {}\n", max));
        }
        out.push_str("  }\n\n");
    }

    fn gen_environment(&self, e: &EnvironmentBlock, out: &mut String) {
        out.push_str("  environment {\n");
        out.push_str(&format!("    inherit = {}\n", e.inherit));
        for (k, v) in &e.variables {
            out.push_str(&format!("    set \"{}\" = \"{}\"\n", k, v));
        }
        for p in &e.passthrough {
            out.push_str(&format!("    passthrough = \"{}\"\n", p));
        }
        out.push_str("  }\n\n");
    }

    fn format_bytes(bytes: u64) -> String {
        if bytes >= 1024 * 1024 * 1024 && bytes % (1024 * 1024 * 1024) == 0 {
            format!("\"{}GB\"", bytes / (1024 * 1024 * 1024))
        } else if bytes >= 1024 * 1024 && bytes % (1024 * 1024) == 0 {
            format!("\"{}MB\"", bytes / (1024 * 1024))
        } else if bytes >= 1024 && bytes % 1024 == 0 {
            format!("\"{}KB\"", bytes / 1024)
        } else {
            format!("{}", bytes)
        }
    }
}

impl Default for PolicyCodegen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_builder::ir::*;

    #[test]
    fn test_generate_basic_policy() {
        let ir = PolicyIR::new("my-sandbox")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())))
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock::default())));

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.contains("sandbox \"my-sandbox\""));
        assert!(hcl.contains("resource {"));
        assert!(hcl.contains("capability {"));
        assert!(hcl.contains("stdout = true"));
    }

    #[test]
    fn test_generate_with_description() {
        let ir = PolicyIR::new("test")
            .with_description("Test policy")
            .add_block(PolicyBlock::new("r", BlockKind::Resource(ResourceBlock::default())));

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.starts_with("# Test policy\n"));
    }

    #[test]
    fn test_generate_network_block() {
        let ir = PolicyIR::new("net-test")
            .add_block(PolicyBlock::new("net", BlockKind::Network(NetworkBlock {
                allow_outbound: true,
                allowed_hosts: vec!["api.example.com".into()],
                allowed_ports: vec![443],
                max_connections: Some(10),
            })));

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.contains("allow_outbound = true"));
        assert!(hcl.contains("allow_host = \"api.example.com\""));
        assert!(hcl.contains("allow_port = 443"));
        assert!(hcl.contains("max_connections = 10"));
    }

    #[test]
    fn test_disabled_block_commented_out() {
        let ir = PolicyIR::new("test")
            .add_block(PolicyBlock::new("disabled", BlockKind::Resource(ResourceBlock::default())).disabled());

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.contains("# Block 'disabled' is disabled"));
        assert!(!hcl.contains("resource {"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(PolicyCodegen::format_bytes(1024 * 1024), "\"1MB\"");
        assert_eq!(PolicyCodegen::format_bytes(64 * 1024 * 1024), "\"64MB\"");
        assert_eq!(PolicyCodegen::format_bytes(1024 * 1024 * 1024), "\"1GB\"");
        assert_eq!(PolicyCodegen::format_bytes(512), "512");
    }

    #[test]
    fn test_generate_environment() {
        let ir = PolicyIR::new("env-test")
            .add_block(PolicyBlock::new("env", BlockKind::Environment(EnvironmentBlock {
                inherit: false,
                variables: vec![("KEY".into(), "value".into())],
                passthrough: vec!["HOME".into()],
            })));

        let codegen = PolicyCodegen::new();
        let hcl = codegen.generate(&ir);
        assert!(hcl.contains("inherit = false"));
        assert!(hcl.contains("set \"KEY\" = \"value\""));
        assert!(hcl.contains("passthrough = \"HOME\""));
    }
}
