//! Cloud provider abstraction.

use serde::{Deserialize, Serialize};

/// Supported cloud providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloudProvider {
    Aws,
    Azure,
    Gcp,
    DigitalOcean,
    OnPrem,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aws => write!(f, "AWS"),
            Self::Azure => write!(f, "Azure"),
            Self::Gcp => write!(f, "GCP"),
            Self::DigitalOcean => write!(f, "DigitalOcean"),
            Self::OnPrem => write!(f, "On-Premise"),
        }
    }
}

impl CloudProvider {
    pub fn from_str_id(s: &str) -> Option<Self> {
        match s {
            "aws" => Some(Self::Aws),
            "azure" => Some(Self::Azure),
            "gcp" => Some(Self::Gcp),
            "digitalocean" | "do" => Some(Self::DigitalOcean),
            "onprem" | "on-prem" => Some(Self::OnPrem),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Azure => "azure",
            Self::Gcp => "gcp",
            Self::DigitalOcean => "digitalocean",
            Self::OnPrem => "onprem",
        }
    }
}

/// Region within a cloud provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegion {
    pub provider: CloudProvider,
    pub region_id: String,
    pub display_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub compliance_tags: Vec<String>,
}

impl ProviderRegion {
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider.id(), self.region_id)
    }
}

/// Capabilities of a provider/region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub gpu_available: bool,
    pub max_memory_mb: u32,
    pub max_vcpus: u32,
    pub spot_available: bool,
    pub wasm_native: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            gpu_available: false,
            max_memory_mb: 8192,
            max_vcpus: 4,
            spot_available: true,
            wasm_native: false,
        }
    }
}

/// Configuration for a cloud provider endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider: CloudProvider,
    pub regions: Vec<ProviderRegion>,
    pub capabilities: ProviderCapabilities,
    pub enabled: bool,
    pub priority: u32,
}

impl ProviderConfig {
    pub fn new(provider: CloudProvider) -> Self {
        Self {
            provider,
            regions: Vec::new(),
            capabilities: ProviderCapabilities::default(),
            enabled: true,
            priority: 0,
        }
    }

    pub fn add_region(mut self, region: ProviderRegion) -> Self {
        self.regions.push(region);
        self
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_from_str() {
        assert_eq!(CloudProvider::from_str_id("aws"), Some(CloudProvider::Aws));
        assert_eq!(CloudProvider::from_str_id("gcp"), Some(CloudProvider::Gcp));
        assert_eq!(CloudProvider::from_str_id("unknown"), None);
    }

    #[test]
    fn test_provider_display() {
        assert_eq!(CloudProvider::Aws.to_string(), "AWS");
        assert_eq!(CloudProvider::OnPrem.to_string(), "On-Premise");
    }

    #[test]
    fn test_region_key() {
        let region = ProviderRegion {
            provider: CloudProvider::Aws,
            region_id: "us-east-1".into(),
            display_name: "US East".into(),
            latitude: 39.0,
            longitude: -77.0,
            compliance_tags: vec!["soc2".into()],
        };
        assert_eq!(region.key(), "aws:us-east-1");
    }

    #[test]
    fn test_provider_config_builder() {
        let config =
            ProviderConfig::new(CloudProvider::Gcp).with_priority(10).add_region(ProviderRegion {
                provider: CloudProvider::Gcp,
                region_id: "us-central1".into(),
                display_name: "Iowa".into(),
                latitude: 41.0,
                longitude: -93.0,
                compliance_tags: vec![],
            });

        assert_eq!(config.provider, CloudProvider::Gcp);
        assert_eq!(config.priority, 10);
        assert_eq!(config.regions.len(), 1);
    }

    #[test]
    fn test_capabilities_default() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.gpu_available);
        assert!(caps.spot_available);
        assert_eq!(caps.max_memory_mb, 8192);
    }
}
