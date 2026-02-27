//! API key management and team RBAC for the control plane.
//!
//! Provides secure API key generation, validation, and team-based
//! role-based access control for multi-tenant sandbox management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

/// An API key with associated metadata and permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique key ID (public, used for lookup).
    pub key_id: String,
    /// Hashed key secret (never store plaintext).
    pub key_hash: String,
    /// Key prefix for display (e.g., "iso_live_abc1...").
    pub prefix: String,
    /// Team this key belongs to.
    pub team_id: Uuid,
    /// Role assigned to this key.
    pub role: Role,
    /// When the key was created.
    pub created_at: DateTime<Utc>,
    /// When the key expires (if ever).
    pub expires_at: Option<DateTime<Utc>>,
    /// Whether the key is active.
    pub active: bool,
    /// Human-readable name.
    pub name: String,
    /// Rate limit (requests per minute).
    pub rate_limit: Option<u32>,
    /// Allowed IP ranges (empty = all).
    pub allowed_ips: Vec<String>,
    /// Last used timestamp.
    pub last_used: Option<DateTime<Utc>>,
    /// Total requests made.
    pub request_count: u64,
}

/// Role-based access level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    /// Read-only access to sandbox outputs and metrics.
    Viewer,
    /// Can create and run sandboxes.
    Developer,
    /// Full access including configuration changes.
    Admin,
    /// Complete control including team management.
    Owner,
}

impl Role {
    /// Check if this role can perform an action.
    pub fn can(&self, action: &Action) -> bool {
        match action {
            Action::ViewMetrics | Action::ViewLogs => true,
            Action::CreateSandbox | Action::RunSandbox => *self >= Role::Developer,
            Action::ManageKeys | Action::ManageConfig => *self >= Role::Admin,
            Action::ManageTeam | Action::ManageBilling => *self >= Role::Owner,
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Viewer => write!(f, "viewer"),
            Role::Developer => write!(f, "developer"),
            Role::Admin => write!(f, "admin"),
            Role::Owner => write!(f, "owner"),
        }
    }
}

/// Actions that can be performed in the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ViewMetrics,
    ViewLogs,
    CreateSandbox,
    RunSandbox,
    ManageKeys,
    ManageConfig,
    ManageTeam,
    ManageBilling,
}

/// A team for organizing users and sandboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Unique team ID.
    pub id: Uuid,
    /// Team name.
    pub name: String,
    /// Team members.
    pub members: Vec<TeamMember>,
    /// When the team was created.
    pub created_at: DateTime<Utc>,
    /// Usage quota.
    pub quota: UsageQuota,
}

/// A team member with a role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// User identifier (email or external ID).
    pub user_id: String,
    /// Role in the team.
    pub role: Role,
    /// When the member joined.
    pub joined_at: DateTime<Utc>,
}

/// Usage quota for a team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageQuota {
    /// Maximum sandboxes per month.
    pub max_sandboxes_per_month: u64,
    /// Maximum CPU-seconds per month.
    pub max_cpu_seconds: u64,
    /// Maximum memory-GB-seconds per month.
    pub max_memory_gb_seconds: u64,
    /// Maximum concurrent sandboxes.
    pub max_concurrent: u32,
}

impl Default for UsageQuota {
    fn default() -> Self {
        Self {
            max_sandboxes_per_month: 10_000,
            max_cpu_seconds: 100_000,
            max_memory_gb_seconds: 500_000,
            max_concurrent: 100,
        }
    }
}

/// Usage tracking for a team.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Team ID.
    pub team_id: Uuid,
    /// Period (month in YYYY-MM format).
    pub period: String,
    /// Total sandbox executions.
    pub sandbox_count: u64,
    /// Total CPU-seconds consumed.
    pub cpu_seconds: f64,
    /// Total memory-GB-seconds consumed.
    pub memory_gb_seconds: f64,
    /// Peak concurrent sandboxes.
    pub peak_concurrent: u32,
    /// Total fuel consumed.
    pub total_fuel: u64,
}

/// API key manager for creating, validating, and revoking keys.
pub struct ApiKeyManager {
    keys: HashMap<String, ApiKey>,
    teams: HashMap<Uuid, Team>,
    usage: HashMap<(Uuid, String), UsageRecord>,
}

impl ApiKeyManager {
    /// Create a new API key manager.
    pub fn new() -> Self {
        Self { keys: HashMap::new(), teams: HashMap::new(), usage: HashMap::new() }
    }

    /// Generate a new API key for a team.
    pub fn generate_key(
        &mut self,
        team_id: Uuid,
        name: impl Into<String>,
        role: Role,
    ) -> (String, ApiKey) {
        let key_id =
            format!("kid_{}", Uuid::new_v4().to_string().replace('-', "")[..16].to_string());
        let secret = format!("iso_live_{}", Uuid::new_v4().to_string().replace('-', ""));
        let prefix = format!("{}...{}", &secret[..12], &secret[secret.len() - 4..]);

        let key_hash = {
            let mut hasher = Sha256::new();
            hasher.update(secret.as_bytes());
            hex::encode(hasher.finalize())
        };

        let api_key = ApiKey {
            key_id: key_id.clone(),
            key_hash,
            prefix,
            team_id,
            role,
            created_at: Utc::now(),
            expires_at: None,
            active: true,
            name: name.into(),
            rate_limit: None,
            allowed_ips: Vec::new(),
            last_used: None,
            request_count: 0,
        };

        self.keys.insert(key_id, api_key.clone());
        (secret, api_key)
    }

    /// Validate an API key by its secret.
    pub fn validate_key(&mut self, secret: &str) -> Option<&ApiKey> {
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(secret.as_bytes());
            hex::encode(hasher.finalize())
        };

        // Find key by hash
        let key_id = self
            .keys
            .iter()
            .find(|(_, k)| k.key_hash == hash && k.active)
            .map(|(id, _)| id.clone())?;

        let key = self.keys.get_mut(&key_id)?;

        // Check expiration
        if let Some(expires) = key.expires_at {
            if Utc::now() > expires {
                key.active = false;
                return None;
            }
        }

        key.last_used = Some(Utc::now());
        key.request_count += 1;

        self.keys.get(&key_id)
    }

    /// Revoke an API key.
    pub fn revoke_key(&mut self, key_id: &str) -> bool {
        if let Some(key) = self.keys.get_mut(key_id) {
            key.active = false;
            true
        } else {
            false
        }
    }

    /// List keys for a team.
    pub fn list_keys(&self, team_id: &Uuid) -> Vec<&ApiKey> {
        self.keys.values().filter(|k| k.team_id == *team_id).collect()
    }

    /// Create a new team.
    pub fn create_team(&mut self, name: impl Into<String>, owner_id: impl Into<String>) -> Team {
        let team = Team {
            id: Uuid::new_v4(),
            name: name.into(),
            members: vec![TeamMember {
                user_id: owner_id.into(),
                role: Role::Owner,
                joined_at: Utc::now(),
            }],
            created_at: Utc::now(),
            quota: UsageQuota::default(),
        };
        self.teams.insert(team.id, team.clone());
        team
    }

    /// Get a team by ID.
    pub fn get_team(&self, id: &Uuid) -> Option<&Team> {
        self.teams.get(id)
    }

    /// Record usage for a team.
    pub fn record_usage(&mut self, team_id: Uuid, period: &str, cpu_seconds: f64, fuel: u64) {
        let record = self.usage.entry((team_id, period.to_string())).or_insert_with(|| {
            UsageRecord { team_id, period: period.to_string(), ..Default::default() }
        });
        record.sandbox_count += 1;
        record.cpu_seconds += cpu_seconds;
        record.total_fuel += fuel;
    }

    /// Get usage for a team in a period.
    pub fn get_usage(&self, team_id: &Uuid, period: &str) -> Option<&UsageRecord> {
        self.usage.get(&(*team_id, period.to_string()))
    }

    /// Check if a team is within its quota.
    pub fn check_quota(&self, team_id: &Uuid, period: &str) -> QuotaStatus {
        let Some(team) = self.teams.get(team_id) else {
            return QuotaStatus {
                within_limits: false,
                reasons: vec!["Team not found".to_string()],
            };
        };

        let usage = self.usage.get(&(*team_id, period.to_string()));
        let mut reasons = Vec::new();

        if let Some(usage) = usage {
            if usage.sandbox_count >= team.quota.max_sandboxes_per_month {
                reasons.push(format!(
                    "Sandbox limit exceeded: {}/{}",
                    usage.sandbox_count, team.quota.max_sandboxes_per_month
                ));
            }
            if usage.cpu_seconds >= team.quota.max_cpu_seconds as f64 {
                reasons.push("CPU-seconds quota exceeded".to_string());
            }
        }

        QuotaStatus { within_limits: reasons.is_empty(), reasons }
    }
}

impl Default for ApiKeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a quota check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    /// Whether the team is within its quota limits.
    pub within_limits: bool,
    /// Reasons for quota exceeded (empty if within limits).
    pub reasons: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_generation() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("Test Team", "user@example.com");

        let (secret, key) = manager.generate_key(team.id, "Test Key", Role::Developer);
        assert!(!secret.is_empty());
        assert!(secret.starts_with("iso_live_"));
        assert_eq!(key.role, Role::Developer);
        assert!(key.active);
    }

    #[test]
    fn test_api_key_validation() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("Test Team", "user@example.com");
        let (secret, _) = manager.generate_key(team.id, "Key", Role::Developer);

        let validated = manager.validate_key(&secret);
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().request_count, 1);

        // Invalid key should fail
        assert!(manager.validate_key("invalid_key").is_none());
    }

    #[test]
    fn test_api_key_revocation() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("Test Team", "user@example.com");
        let (secret, key) = manager.generate_key(team.id, "Key", Role::Developer);

        assert!(manager.revoke_key(&key.key_id));
        assert!(manager.validate_key(&secret).is_none());
    }

    #[test]
    fn test_role_permissions() {
        assert!(Role::Viewer.can(&Action::ViewMetrics));
        assert!(!Role::Viewer.can(&Action::CreateSandbox));
        assert!(Role::Developer.can(&Action::CreateSandbox));
        assert!(!Role::Developer.can(&Action::ManageKeys));
        assert!(Role::Admin.can(&Action::ManageKeys));
        assert!(!Role::Admin.can(&Action::ManageTeam));
        assert!(Role::Owner.can(&Action::ManageTeam));
    }

    #[test]
    fn test_team_creation() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("My Team", "owner@example.com");

        assert_eq!(team.name, "My Team");
        assert_eq!(team.members.len(), 1);
        assert_eq!(team.members[0].role, Role::Owner);

        let retrieved = manager.get_team(&team.id);
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_usage_tracking() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("Team", "user@test.com");

        manager.record_usage(team.id, "2026-02", 10.5, 1_000_000);
        manager.record_usage(team.id, "2026-02", 5.0, 500_000);

        let usage = manager.get_usage(&team.id, "2026-02").unwrap();
        assert_eq!(usage.sandbox_count, 2);
        assert!((usage.cpu_seconds - 15.5).abs() < 0.01);
        assert_eq!(usage.total_fuel, 1_500_000);
    }

    #[test]
    fn test_quota_check() {
        let mut manager = ApiKeyManager::new();
        let team = manager.create_team("Team", "user@test.com");

        let status = manager.check_quota(&team.id, "2026-02");
        assert!(status.within_limits);

        // Record up to the limit
        for _ in 0..10_000 {
            manager.record_usage(team.id, "2026-02", 0.1, 100);
        }

        let status = manager.check_quota(&team.id, "2026-02");
        assert!(!status.within_limits);
    }

    #[test]
    fn test_role_display() {
        assert_eq!(Role::Viewer.to_string(), "viewer");
        assert_eq!(Role::Admin.to_string(), "admin");
        assert_eq!(Role::Owner.to_string(), "owner");
    }
}
