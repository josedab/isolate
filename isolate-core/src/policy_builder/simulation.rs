//! Policy simulation engine.

use serde::{Deserialize, Serialize};

use super::ir::{BlockKind, CapabilityBlock, NetworkBlock, PolicyIR};

/// A simulated action to test against a policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimulatedAction {
    WriteStdout,
    WriteStderr,
    ReadStdin,
    ReadFile(String),
    WriteFile(String),
    NetworkConnect(String, u16),
    AllocateMemory(u64),
    ReadEnvVar(String),
}

/// Result of a simulation check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub action: String,
    pub allowed: bool,
    pub reason: String,
}

/// Result of simulating a set of actions against a policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub policy_name: String,
    pub results: Vec<ActionResult>,
    pub all_allowed: bool,
    pub denied_count: usize,
}

/// Simulates policy enforcement without running actual sandboxes.
pub struct PolicySimulator;

impl PolicySimulator {
    pub fn new() -> Self {
        Self
    }

    /// Simulate a list of actions against a policy IR.
    pub fn simulate(&self, ir: &PolicyIR, actions: &[SimulatedAction]) -> SimulationResult {
        let caps = self.find_capability(ir);
        let net = self.find_network(ir);

        let mut results = Vec::new();
        let mut denied = 0;

        for action in actions {
            let (allowed, reason) = self.check_action(action, caps, net, ir);
            if !allowed {
                denied += 1;
            }
            results.push(ActionResult {
                action: format!("{:?}", action),
                allowed,
                reason,
            });
        }

        SimulationResult {
            policy_name: ir.name.clone(),
            results,
            all_allowed: denied == 0,
            denied_count: denied,
        }
    }

    fn find_capability<'a>(&self, ir: &'a PolicyIR) -> Option<&'a CapabilityBlock> {
        ir.blocks.iter()
            .filter(|b| b.enabled)
            .find_map(|b| match &b.kind {
                BlockKind::Capability(c) => Some(c),
                _ => None,
            })
    }

    fn find_network<'a>(&self, ir: &'a PolicyIR) -> Option<&'a NetworkBlock> {
        ir.blocks.iter()
            .filter(|b| b.enabled)
            .find_map(|b| match &b.kind {
                BlockKind::Network(n) => Some(n),
                _ => None,
            })
    }

    fn check_action(
        &self,
        action: &SimulatedAction,
        caps: Option<&CapabilityBlock>,
        net: Option<&NetworkBlock>,
        ir: &PolicyIR,
    ) -> (bool, String) {
        let caps = match caps {
            Some(c) => c,
            None => return (false, "No capability block defined".into()),
        };

        match action {
            SimulatedAction::WriteStdout => {
                (caps.stdout, if caps.stdout { "stdout enabled" } else { "stdout not granted" }.into())
            }
            SimulatedAction::WriteStderr => {
                (caps.stderr, if caps.stderr { "stderr enabled" } else { "stderr not granted" }.into())
            }
            SimulatedAction::ReadStdin => {
                (caps.stdin, if caps.stdin { "stdin enabled" } else { "stdin not granted" }.into())
            }
            SimulatedAction::ReadFile(path) => {
                let allowed = caps.filesystem_read.iter().any(|p| path.starts_with(p));
                let reason = if allowed {
                    "Path matches read permission".to_string()
                } else {
                    format!("No read permission for {}", path)
                };
                (allowed, reason)
            }
            SimulatedAction::WriteFile(path) => {
                let allowed = caps.filesystem_write.iter().any(|p| path.starts_with(p));
                let reason = if allowed {
                    "Path matches write permission".to_string()
                } else {
                    format!("No write permission for {}", path)
                };
                (allowed, reason)
            }
            SimulatedAction::NetworkConnect(host, port) => {
                let net = match net {
                    Some(n) => n,
                    None => return (false, "No network block defined".into()),
                };
                if !net.allow_outbound {
                    return (false, "Outbound network disabled".into());
                }
                let host_ok = net.allowed_hosts.is_empty() || net.allowed_hosts.iter().any(|h| h == host);
                let port_ok = net.allowed_ports.is_empty() || net.allowed_ports.contains(port);
                let allowed = host_ok && port_ok;
                let reason = if allowed {
                    "Network access permitted".into()
                } else if !host_ok {
                    format!("Host {} not in allowed list", host)
                } else {
                    format!("Port {} not in allowed list", port)
                };
                (allowed, reason)
            }
            SimulatedAction::AllocateMemory(bytes) => {
                let max = ir.blocks.iter()
                    .filter(|b| b.enabled)
                    .find_map(|b| match &b.kind {
                        BlockKind::Resource(r) => r.max_memory_bytes,
                        _ => None,
                    });
                match max {
                    Some(m) if *bytes <= m => (true, format!("Within memory limit ({})", m)),
                    Some(m) => (false, format!("Exceeds memory limit ({} > {})", bytes, m)),
                    None => (true, "No memory limit set".into()),
                }
            }
            SimulatedAction::ReadEnvVar(var) => {
                let allowed = caps.env_vars.iter().any(|v| v == var);
                (allowed, if allowed { "Env var access granted".into() } else { format!("Env var {} not in allowed list", var) })
            }
        }
    }
}

impl Default for PolicySimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_builder::ir::*;

    fn web_policy() -> PolicyIR {
        PolicyIR::new("web")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock {
                max_memory_bytes: Some(64 * 1024 * 1024),
                ..Default::default()
            })))
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock {
                stdout: true,
                stderr: true,
                stdin: false,
                filesystem_read: vec!["/data".into()],
                filesystem_write: vec![],
                env_vars: vec!["API_KEY".into()],
            })))
            .add_block(PolicyBlock::new("net", BlockKind::Network(NetworkBlock {
                allow_outbound: true,
                allowed_hosts: vec!["api.example.com".into()],
                allowed_ports: vec![443],
                ..Default::default()
            })))
    }

    #[test]
    fn test_simulate_allowed_actions() {
        let sim = PolicySimulator::new();
        let result = sim.simulate(&web_policy(), &[
            SimulatedAction::WriteStdout,
            SimulatedAction::WriteStderr,
            SimulatedAction::ReadFile("/data/input.json".into()),
            SimulatedAction::NetworkConnect("api.example.com".into(), 443),
            SimulatedAction::ReadEnvVar("API_KEY".into()),
        ]);
        assert!(result.all_allowed);
        assert_eq!(result.denied_count, 0);
    }

    #[test]
    fn test_simulate_denied_actions() {
        let sim = PolicySimulator::new();
        let result = sim.simulate(&web_policy(), &[
            SimulatedAction::ReadStdin,
            SimulatedAction::WriteFile("/etc/passwd".into()),
            SimulatedAction::NetworkConnect("evil.com".into(), 443),
            SimulatedAction::ReadEnvVar("SECRET".into()),
        ]);
        assert!(!result.all_allowed);
        assert_eq!(result.denied_count, 4);
    }

    #[test]
    fn test_simulate_memory_limit() {
        let sim = PolicySimulator::new();
        let policy = web_policy();

        let ok = sim.simulate(&policy, &[SimulatedAction::AllocateMemory(32 * 1024 * 1024)]);
        assert!(ok.all_allowed);

        let fail = sim.simulate(&policy, &[SimulatedAction::AllocateMemory(128 * 1024 * 1024)]);
        assert!(!fail.all_allowed);
    }

    #[test]
    fn test_simulate_no_capability_block() {
        let sim = PolicySimulator::new();
        let ir = PolicyIR::new("bare")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())));

        let result = sim.simulate(&ir, &[SimulatedAction::WriteStdout]);
        assert!(!result.all_allowed);
    }

    #[test]
    fn test_simulate_network_disabled() {
        let sim = PolicySimulator::new();
        let ir = PolicyIR::new("no-net")
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock::default())))
            .add_block(PolicyBlock::new("net", BlockKind::Network(NetworkBlock {
                allow_outbound: false,
                ..Default::default()
            })));

        let result = sim.simulate(&ir, &[SimulatedAction::NetworkConnect("example.com".into(), 80)]);
        assert!(!result.all_allowed);
        assert!(result.results[0].reason.contains("disabled"));
    }

    #[test]
    fn test_simulation_result_metadata() {
        let sim = PolicySimulator::new();
        let result = sim.simulate(&web_policy(), &[SimulatedAction::WriteStdout]);
        assert_eq!(result.policy_name, "web");
        assert_eq!(result.results.len(), 1);
    }
}
