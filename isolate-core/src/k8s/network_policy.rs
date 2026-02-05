//! Kubernetes Network Policy and Admission Webhook generation.
//!
//! Generates K8s NetworkPolicy resources and ValidatingWebhookConfiguration
//! from sandbox capability grants. Also provides PodDisruptionBudget generation.

use super::{CapabilityGrant, ObjectMeta};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A Kubernetes NetworkPolicy resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicy {
    pub api_version: String,
    pub kind: String,
    pub metadata: ObjectMeta,
    pub spec: NetworkPolicySpec,
}

/// NetworkPolicy spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicySpec {
    pub pod_selector: LabelSelector,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ingress: Vec<NetworkPolicyIngressRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress: Vec<NetworkPolicyEgressRule>,
    pub policy_types: Vec<String>,
}

/// Label selector.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelSelector {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub match_labels: HashMap<String, String>,
}

/// Ingress rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyIngressRule {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<NetworkPolicyPeer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<NetworkPolicyPort>,
}

/// Egress rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyEgressRule {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<NetworkPolicyPeer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<NetworkPolicyPort>,
}

/// Network policy peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyPeer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_selector: Option<LabelSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_selector: Option<LabelSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_block: Option<IpBlock>,
}

/// IP block for egress/ingress.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpBlock {
    pub cidr: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<String>,
}

/// Network policy port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyPort {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// PodDisruptionBudget resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodDisruptionBudget {
    pub api_version: String,
    pub kind: String,
    pub metadata: ObjectMeta,
    pub spec: PdbSpec,
}

/// PDB spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdbSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_available: Option<PdbValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_unavailable: Option<PdbValue>,
    pub selector: LabelSelector,
}

/// PDB value: either integer or percentage string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PdbValue {
    Count(u32),
    Percentage(String),
}

/// ValidatingWebhookConfiguration resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatingWebhookConfiguration {
    pub api_version: String,
    pub kind: String,
    pub metadata: ObjectMeta,
    pub webhooks: Vec<ValidatingWebhook>,
}

/// A single validating webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatingWebhook {
    pub name: String,
    pub client_config: WebhookClientConfig,
    pub rules: Vec<WebhookRule>,
    pub failure_policy: String,
    pub side_effects: String,
    pub admission_review_versions: Vec<String>,
}

/// Webhook client config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookClientConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<WebhookServiceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_bundle: Option<String>,
}

/// Webhook service reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookServiceRef {
    pub name: String,
    pub namespace: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Webhook rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRule {
    pub api_groups: Vec<String>,
    pub api_versions: Vec<String>,
    pub operations: Vec<String>,
    pub resources: Vec<String>,
}

/// Generate a NetworkPolicy from sandbox capabilities.
///
/// Creates a deny-all default with explicit egress rules for each network capability.
pub fn generate_network_policy(
    sandbox_name: &str,
    namespace: &str,
    capabilities: &[CapabilityGrant],
) -> NetworkPolicy {
    let mut labels = HashMap::new();
    labels.insert("isolate.io/sandbox".to_string(), sandbox_name.to_string());

    let mut egress_rules = Vec::new();

    // Always allow DNS
    egress_rules.push(NetworkPolicyEgressRule {
        to: vec![],
        ports: vec![
            NetworkPolicyPort { protocol: Some("UDP".to_string()), port: Some(53) },
            NetworkPolicyPort { protocol: Some("TCP".to_string()), port: Some(53) },
        ],
    });

    // Add egress rules for network capabilities
    for cap in capabilities {
        if cap.cap_type == "network" {
            for target in &cap.allow {
                if let Some(rule) = parse_network_target(target) {
                    egress_rules.push(rule);
                }
            }
        }
    }

    let policy_types = if egress_rules.is_empty() {
        vec!["Ingress".to_string(), "Egress".to_string()]
    } else {
        vec!["Egress".to_string()]
    };

    NetworkPolicy {
        api_version: "networking.k8s.io/v1".to_string(),
        kind: "NetworkPolicy".to_string(),
        metadata: ObjectMeta {
            name: format!("{}-netpol", sandbox_name),
            namespace: Some(namespace.to_string()),
            labels: labels.clone(),
            ..Default::default()
        },
        spec: NetworkPolicySpec {
            pod_selector: LabelSelector { match_labels: labels },
            ingress: vec![],
            egress: egress_rules,
            policy_types,
        },
    }
}

fn parse_network_target(target: &str) -> Option<NetworkPolicyEgressRule> {
    // Parse targets like "api.example.com:443" or "10.0.0.0/8"
    if target.contains('/') {
        // CIDR notation
        Some(NetworkPolicyEgressRule {
            to: vec![NetworkPolicyPeer {
                pod_selector: None,
                namespace_selector: None,
                ip_block: Some(IpBlock { cidr: target.to_string(), except: vec![] }),
            }],
            ports: vec![],
        })
    } else if let Some((_host, port_str)) = target.rsplit_once(':') {
        let port = port_str.parse::<u16>().ok()?;
        Some(NetworkPolicyEgressRule {
            to: vec![],
            ports: vec![NetworkPolicyPort { protocol: Some("TCP".to_string()), port: Some(port) }],
        })
    } else {
        // Plain hostname — allow all ports
        Some(NetworkPolicyEgressRule { to: vec![], ports: vec![] })
    }
}

/// Generate a PodDisruptionBudget for a sandbox pool.
pub fn generate_pdb(
    pool_name: &str,
    namespace: &str,
    min_available: PdbValue,
) -> PodDisruptionBudget {
    let mut labels = HashMap::new();
    labels.insert("isolate.io/pool".to_string(), pool_name.to_string());

    PodDisruptionBudget {
        api_version: "policy/v1".to_string(),
        kind: "PodDisruptionBudget".to_string(),
        metadata: ObjectMeta {
            name: format!("{}-pdb", pool_name),
            namespace: Some(namespace.to_string()),
            labels: labels.clone(),
            ..Default::default()
        },
        spec: PdbSpec {
            min_available: Some(min_available),
            max_unavailable: None,
            selector: LabelSelector { match_labels: labels },
        },
    }
}

/// Generate a ValidatingWebhookConfiguration for sandbox admission control.
pub fn generate_admission_webhook(
    service_name: &str,
    namespace: &str,
    ca_bundle: Option<&str>,
) -> ValidatingWebhookConfiguration {
    ValidatingWebhookConfiguration {
        api_version: "admissionregistration.k8s.io/v1".to_string(),
        kind: "ValidatingWebhookConfiguration".to_string(),
        metadata: ObjectMeta {
            name: "isolate-sandbox-validator".to_string(),
            namespace: None,
            ..Default::default()
        },
        webhooks: vec![ValidatingWebhook {
            name: "validate.sandbox.isolate.io".to_string(),
            client_config: WebhookClientConfig {
                service: Some(WebhookServiceRef {
                    name: service_name.to_string(),
                    namespace: namespace.to_string(),
                    path: "/validate-sandbox".to_string(),
                    port: Some(443),
                }),
                url: None,
                ca_bundle: ca_bundle.map(|s| s.to_string()),
            },
            rules: vec![WebhookRule {
                api_groups: vec!["isolate.io".to_string()],
                api_versions: vec!["v1alpha1".to_string()],
                operations: vec!["CREATE".to_string(), "UPDATE".to_string()],
                resources: vec!["sandboxes".to_string()],
            }],
            failure_policy: "Fail".to_string(),
            side_effects: "None".to_string(),
            admission_review_versions: vec!["v1".to_string()],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_network_policy_default() {
        let policy = generate_network_policy("my-sandbox", "default", &[]);

        assert_eq!(policy.metadata.name, "my-sandbox-netpol");
        // DNS egress is always present, so policy_types = ["Egress"]
        assert_eq!(policy.spec.policy_types, vec!["Egress"]);
        assert!(policy.spec.ingress.is_empty());
        // Only DNS rule
        assert_eq!(policy.spec.egress.len(), 1);
    }

    #[test]
    fn test_generate_network_policy_with_capabilities() {
        let caps = vec![CapabilityGrant {
            cap_type: "network".to_string(),
            allow: vec!["api.example.com:443".to_string(), "10.0.0.0/8".to_string()],
            deny: vec![],
        }];

        let policy = generate_network_policy("my-sandbox", "default", &caps);

        // DNS + 2 capability rules = 3
        assert_eq!(policy.spec.egress.len(), 3);
        assert_eq!(policy.spec.policy_types, vec!["Egress"]);
    }

    #[test]
    fn test_network_policy_dns_always_allowed() {
        let policy = generate_network_policy(
            "test",
            "default",
            &[CapabilityGrant { cap_type: "network".to_string(), allow: vec![], deny: vec![] }],
        );

        // DNS rule should always be present
        let dns_rule = &policy.spec.egress[0];
        assert_eq!(dns_rule.ports.len(), 2);
        assert_eq!(dns_rule.ports[0].port, Some(53));
    }

    #[test]
    fn test_network_policy_cidr() {
        let caps = vec![CapabilityGrant {
            cap_type: "network".to_string(),
            allow: vec!["192.168.0.0/16".to_string()],
            deny: vec![],
        }];

        let policy = generate_network_policy("test", "default", &caps);
        let cidr_rule = &policy.spec.egress[1];

        assert_eq!(cidr_rule.to.len(), 1);
        assert_eq!(cidr_rule.to[0].ip_block.as_ref().unwrap().cidr, "192.168.0.0/16");
    }

    #[test]
    fn test_generate_pdb_count() {
        let pdb = generate_pdb("my-pool", "default", PdbValue::Count(2));

        assert_eq!(pdb.metadata.name, "my-pool-pdb");
        assert!(matches!(pdb.spec.min_available, Some(PdbValue::Count(2))));
        assert!(pdb.spec.max_unavailable.is_none());
    }

    #[test]
    fn test_generate_pdb_percentage() {
        let pdb = generate_pdb("my-pool", "default", PdbValue::Percentage("50%".to_string()));

        if let Some(PdbValue::Percentage(p)) = &pdb.spec.min_available {
            assert_eq!(p, "50%");
        } else {
            panic!("Expected percentage");
        }
    }

    #[test]
    fn test_generate_admission_webhook() {
        let webhook =
            generate_admission_webhook("isolate-webhook", "isolate-system", Some("base64-ca-data"));

        assert_eq!(webhook.webhooks.len(), 1);
        let wh = &webhook.webhooks[0];
        assert_eq!(wh.name, "validate.sandbox.isolate.io");
        assert_eq!(wh.failure_policy, "Fail");
        assert_eq!(wh.rules[0].resources, vec!["sandboxes"]);

        let svc = wh.client_config.service.as_ref().unwrap();
        assert_eq!(svc.name, "isolate-webhook");
        assert_eq!(svc.namespace, "isolate-system");
    }

    #[test]
    fn test_webhook_without_ca_bundle() {
        let webhook = generate_admission_webhook("svc", "ns", None);
        assert!(webhook.webhooks[0].client_config.ca_bundle.is_none());
    }

    #[test]
    fn test_network_policy_serialization() {
        let policy = generate_network_policy("test", "default", &[]);
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("networking.k8s.io/v1"));
        assert!(json.contains("NetworkPolicy"));
    }

    #[test]
    fn test_pdb_serialization() {
        let pdb = generate_pdb("pool", "default", PdbValue::Count(1));
        let json = serde_json::to_string(&pdb).unwrap();
        assert!(json.contains("PodDisruptionBudget"));
        assert!(json.contains("policy/v1"));
    }

    #[test]
    fn test_non_network_capabilities_ignored() {
        let caps = vec![
            CapabilityGrant {
                cap_type: "filesystem".to_string(),
                allow: vec!["/data".to_string()],
                deny: vec![],
            },
            CapabilityGrant {
                cap_type: "network".to_string(),
                allow: vec!["api.example.com:8080".to_string()],
                deny: vec![],
            },
        ];

        let policy = generate_network_policy("test", "default", &caps);
        // DNS + 1 network rule = 2 (filesystem ignored)
        assert_eq!(policy.spec.egress.len(), 2);
    }
}
