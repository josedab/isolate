//! Capability System Example
//!
//! This example demonstrates Isolate's capability-based security model:
//! - Default-deny access control
//! - Granting filesystem read/write capabilities
//! - Network capabilities for HTTP clients
//! - Environment variable access
//! - Clock and timer access

use isolate_core::{capability::Capability, SandboxConfig};
use std::time::Duration;

// A minimal WASM module for demonstration
const MINIMAL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic: \0asm
    0x01, 0x00, 0x00, 0x00, // version: 1
];

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    println!("Isolate Capability System Example");
    println!("===================================\n");

    // Example 1: Default deny - no capabilities granted
    println!("1. Default Deny Configuration:");
    let config_deny = SandboxConfig::builder().module(MINIMAL_WASM)?.build()?;

    println!("   Capabilities: none (default deny)");
    println!("   - No stdout/stderr access");
    println!("   - No filesystem access");
    println!("   - No network access");
    println!();

    // Example 2: Standard I/O capabilities
    println!("2. Standard I/O Configuration:");
    let config_stdio = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .capability(Capability::stdin())
        .build()?;

    println!("   Granted: stdout, stderr, stdin");
    println!();

    // Example 3: Filesystem capabilities with path restrictions
    println!("3. Filesystem Configuration:");
    let config_fs = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // Read-only access to specific directories
        .capability(Capability::filesystem_read("/data/config"))
        .capability(Capability::filesystem_read("/usr/share/data"))
        // Read-write access to a working directory
        .capability(Capability::filesystem_write("/tmp/sandbox-work"))
        // Temporary directory access
        .capability(Capability::temp_dir())
        .build()?;

    println!("   Granted filesystem access:");
    println!("   - Read: /data/config, /usr/share/data");
    println!("   - Write: /tmp/sandbox-work");
    println!("   - Temp: system temp directory");
    println!();

    // Example 4: Network capabilities
    println!("4. Network Configuration:");
    let config_network = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // HTTP client to specific hosts
        .capability(Capability::http_client(vec![
            "api.example.com".to_string(),
            "cdn.example.com".to_string(),
        ]))
        // Wildcard domain matching
        .capability(Capability::http_client(vec![
            "*.internal.example.com".to_string()
        ]))
        // DNS resolution
        .capability(Capability::dns_resolve())
        .build()?;

    println!("   Granted network access:");
    println!("   - HTTP: api.example.com, cdn.example.com");
    println!("   - HTTP: *.internal.example.com (wildcard)");
    println!("   - DNS resolution enabled");
    println!();

    // Example 5: Environment and clock capabilities
    println!("5. Environment and Clock Configuration:");
    let config_env = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        .capability(Capability::stdout())
        // Specific environment variables only
        .capability(Capability::env_var("API_KEY"))
        .capability(Capability::env_var("LOG_LEVEL"))
        // Clock access
        .capability(Capability::system_clock())
        .capability(Capability::monotonic_clock())
        // Secure random
        .capability(Capability::secure_random())
        .build()?;

    println!("   Granted environment access:");
    println!("   - Env vars: API_KEY, LOG_LEVEL");
    println!("   - System and monotonic clocks");
    println!("   - Secure random number generation");
    println!();

    // Example 6: Full production configuration
    println!("6. Production Configuration Example:");
    let _config_production = SandboxConfig::builder()
        .module(MINIMAL_WASM)?
        // Resource limits
        .memory_limit(256 * 1024 * 1024) // 256MB
        .fuel(100_000_000) // 100M instructions
        .wall_time_limit(Duration::from_secs(60))
        // I/O capabilities
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        // Filesystem (read-only config, write to workspace)
        .capability(Capability::filesystem_read("/etc/app"))
        .capability(Capability::filesystem_write("/var/app/data"))
        .capability(Capability::temp_dir())
        // Network (API access only)
        .capability(Capability::http_client(vec![
            "api.service.internal".to_string()
        ]))
        .capability(Capability::dns_resolve())
        // Environment
        .capability(Capability::env_var("APP_ENV"))
        .capability(Capability::system_clock())
        .capability(Capability::secure_random())
        .build()?;

    println!("   A production-ready configuration with:");
    println!("   - Resource limits enforced");
    println!("   - Minimal filesystem access");
    println!("   - Restricted network to internal API");
    println!("   - Limited environment exposure");
    println!();

    println!("Example complete!");
    println!("\nNote: Capabilities are checked at runtime by the CapabilityEnforcer.");
    println!("Any attempt to access unpermitted resources will be denied and logged.");

    // Prevent unused variable warning
    let _ = config_deny;
    let _ = config_stdio;
    let _ = config_fs;
    let _ = config_network;
    let _ = config_env;

    Ok(())
}
