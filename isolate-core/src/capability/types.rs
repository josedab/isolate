//! Capability type definitions.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

/// A capability that can be granted to a sandbox.
///
/// Capabilities define what resources and operations a sandbox is allowed to access.
/// By default, a sandbox has no capabilities (default-deny). You must explicitly
/// grant each capability needed.
///
/// # Examples
///
/// ```
/// use isolate_core::capability::Capability;
///
/// // Grant stdout and stderr
/// let stdout = Capability::stdout();
/// let stderr = Capability::stderr();
///
/// // Grant filesystem read access
/// let fs_read = Capability::filesystem_read("/data");
///
/// // Grant HTTP access to specific hosts
/// let http = Capability::http_client(vec!["api.example.com"]);
///
/// // Check capability descriptions
/// assert_eq!(stdout.to_string(), "stdio:stdout");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Capability {
    /// Filesystem access capability.
    Filesystem(FilesystemCapability),
    /// Network access capability.
    Network(NetworkCapability),
    /// Time access capability.
    Time(TimeCapability),
    /// Random number generation capability.
    Random(RandomCapability),
    /// Environment access capability.
    Environment(EnvironmentCapability),
    /// Standard I/O capability.
    Stdio(StdioCapability),
    /// Host function call capability.
    HostFunction(HostFunctionCapability),
}

impl Capability {
    // Convenience constructors

    /// Create a stdout capability.
    pub fn stdout() -> Self {
        Self::Stdio(StdioCapability::Stdout)
    }

    /// Create a stderr capability.
    pub fn stderr() -> Self {
        Self::Stdio(StdioCapability::Stderr)
    }

    /// Create a stdin capability.
    pub fn stdin() -> Self {
        Self::Stdio(StdioCapability::Stdin)
    }

    /// Create a read-only filesystem capability.
    pub fn filesystem_read(path: impl Into<PathBuf>) -> Self {
        Self::Filesystem(FilesystemCapability::ReadOnly(path.into()))
    }

    /// Create a read-write filesystem capability.
    pub fn filesystem_write(path: impl Into<PathBuf>) -> Self {
        Self::Filesystem(FilesystemCapability::ReadWrite(path.into()))
    }

    /// Create a temporary directory capability.
    pub fn temp_dir() -> Self {
        Self::Filesystem(FilesystemCapability::TempDir)
    }

    /// Create an HTTP client capability.
    pub fn http_client(hosts: Vec<impl Into<String>>) -> Self {
        Self::Network(NetworkCapability::HttpClient(hosts.into_iter().map(Into::into).collect()))
    }

    /// Create a TCP connect capability.
    pub fn tcp_connect(addrs: Vec<SocketAddr>) -> Self {
        Self::Network(NetworkCapability::TcpConnect(addrs))
    }

    /// Create a TCP listen capability.
    pub fn tcp_listen(port: u16) -> Self {
        Self::Network(NetworkCapability::TcpListen(port))
    }

    /// Create a DNS resolution capability.
    pub fn dns_resolve() -> Self {
        Self::Network(NetworkCapability::DnsResolve)
    }

    /// Create a DNS resolution capability restricted to specific resolvers.
    pub fn dns_resolve_restricted(resolvers: Vec<std::net::IpAddr>) -> Self {
        Self::Network(NetworkCapability::DnsResolveRestricted(resolvers))
    }

    /// Create a system clock access capability.
    pub fn system_clock() -> Self {
        Self::Time(TimeCapability::SystemClock)
    }

    /// Create a monotonic clock access capability.
    pub fn monotonic_clock() -> Self {
        Self::Time(TimeCapability::MonotonicClock)
    }

    /// Create a timer capability.
    pub fn timers() -> Self {
        Self::Time(TimeCapability::Timers)
    }

    /// Create a secure random capability.
    pub fn secure_random() -> Self {
        Self::Random(RandomCapability::Secure)
    }

    /// Create a seeded random capability.
    pub fn seeded_random(seed: u64) -> Self {
        Self::Random(RandomCapability::Seeded(seed))
    }

    /// Create an environment variable read capability.
    pub fn env_var(name: impl Into<String>) -> Self {
        Self::Environment(EnvironmentCapability::ReadVar(name.into()))
    }

    /// Create an all environment variables read capability.
    pub fn env_all() -> Self {
        Self::Environment(EnvironmentCapability::ReadAll)
    }

    /// Create a command-line arguments capability.
    pub fn args() -> Self {
        Self::Environment(EnvironmentCapability::Args)
    }

    /// Create a host function capability.
    pub fn host_function(name: impl Into<String>) -> Self {
        Self::HostFunction(HostFunctionCapability::Named(name.into()))
    }

    /// Get a human-readable description of the capability.
    pub fn description(&self) -> String {
        match self {
            Self::Filesystem(fs) => fs.description(),
            Self::Network(net) => net.description(),
            Self::Time(time) => time.description(),
            Self::Random(rand) => rand.description(),
            Self::Environment(env) => env.description(),
            Self::Stdio(stdio) => stdio.description(),
            Self::HostFunction(hf) => hf.description(),
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Filesystem access capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FilesystemCapability {
    /// Read-only access to a specific path.
    ReadOnly(PathBuf),
    /// Read-write access to a specific path.
    ReadWrite(PathBuf),
    /// Access to a temporary directory.
    TempDir,
}

impl FilesystemCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::ReadOnly(path) => format!("fs:read:{}", path.display()),
            Self::ReadWrite(path) => format!("fs:readwrite:{}", path.display()),
            Self::TempDir => "fs:tempdir".to_string(),
        }
    }

    /// Check if this capability allows reading the given path.
    pub fn allows_read(&self, path: &std::path::Path) -> bool {
        match self {
            Self::ReadOnly(allowed) | Self::ReadWrite(allowed) => path.starts_with(allowed),
            Self::TempDir => false, // Temp dir paths are handled specially
        }
    }

    /// Check if this capability allows writing to the given path.
    pub fn allows_write(&self, path: &std::path::Path) -> bool {
        match self {
            Self::ReadWrite(allowed) => path.starts_with(allowed),
            Self::ReadOnly(_) => false,
            Self::TempDir => false, // Temp dir paths are handled specially
        }
    }
}

/// Network access capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NetworkCapability {
    /// HTTP client access to specific hosts (glob patterns supported).
    HttpClient(Vec<String>),
    /// TCP connection to specific addresses.
    TcpConnect(Vec<SocketAddr>),
    /// TCP listener on a specific port.
    TcpListen(u16),
    /// DNS resolution.
    DnsResolve,
    /// DNS resolution restricted to specific resolver addresses.
    DnsResolveRestricted(Vec<std::net::IpAddr>),
}

impl NetworkCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::HttpClient(hosts) => format!("net:http:{}", hosts.join(",")),
            Self::TcpConnect(addrs) => {
                let addrs: Vec<_> = addrs.iter().map(|a| a.to_string()).collect();
                format!("net:tcp:connect:{}", addrs.join(","))
            }
            Self::TcpListen(port) => format!("net:tcp:listen:{}", port),
            Self::DnsResolve => "net:dns".to_string(),
            Self::DnsResolveRestricted(resolvers) => {
                let addrs: Vec<_> = resolvers.iter().map(|a| a.to_string()).collect();
                format!("net:dns:{}", addrs.join(","))
            }
        }
    }

    /// Check if this capability allows connecting to the given host.
    pub fn allows_http_host(&self, host: &str) -> bool {
        match self {
            Self::HttpClient(allowed) => allowed.iter().any(|pattern| {
                if pattern.starts_with("*.") {
                    // Wildcard subdomain pattern
                    let suffix = &pattern[1..];
                    host.ends_with(suffix) || host == &pattern[2..]
                } else {
                    host == pattern
                }
            }),
            _ => false,
        }
    }

    /// Check if this capability allows connecting to the given address.
    pub fn allows_tcp_connect(&self, addr: &SocketAddr) -> bool {
        match self {
            Self::TcpConnect(allowed) => allowed.contains(addr),
            _ => false,
        }
    }
}

/// Time access capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TimeCapability {
    /// System clock access (wall clock time).
    SystemClock,
    /// Monotonic clock only (for measuring durations).
    MonotonicClock,
    /// Timer creation (sleep, intervals).
    Timers,
}

impl TimeCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::SystemClock => "time:system".to_string(),
            Self::MonotonicClock => "time:monotonic".to_string(),
            Self::Timers => "time:timers".to_string(),
        }
    }
}

/// Random number generation capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RandomCapability {
    /// Cryptographically secure random.
    Secure,
    /// Seeded (deterministic) random with the given seed.
    Seeded(u64),
}

impl RandomCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::Secure => "random:secure".to_string(),
            Self::Seeded(seed) => format!("random:seeded:{}", seed),
        }
    }
}

/// Environment access capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EnvironmentCapability {
    /// Read a specific environment variable.
    ReadVar(String),
    /// Read all environment variables.
    ReadAll,
    /// Read command-line arguments.
    Args,
}

impl EnvironmentCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::ReadVar(name) => format!("env:var:{}", name),
            Self::ReadAll => "env:all".to_string(),
            Self::Args => "env:args".to_string(),
        }
    }

    /// Check if this capability allows reading the given variable.
    pub fn allows_var(&self, name: &str) -> bool {
        match self {
            Self::ReadVar(allowed) => allowed == name,
            Self::ReadAll => true,
            Self::Args => false,
        }
    }
}

/// Standard I/O capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StdioCapability {
    /// Standard input.
    Stdin,
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

impl StdioCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::Stdin => "stdio:stdin".to_string(),
            Self::Stdout => "stdio:stdout".to_string(),
            Self::Stderr => "stdio:stderr".to_string(),
        }
    }
}

/// Host function call capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HostFunctionCapability {
    /// Access to a specific named host function.
    Named(String),
    /// Access to all host functions in a namespace.
    Namespace(String),
}

impl HostFunctionCapability {
    /// Get a description of this capability.
    pub fn description(&self) -> String {
        match self {
            Self::Named(name) => format!("hostfn:{}", name),
            Self::Namespace(ns) => format!("hostfn:{}:*", ns),
        }
    }

    /// Check if this capability allows calling the given function.
    pub fn allows_function(&self, name: &str) -> bool {
        match self {
            Self::Named(allowed) => allowed == name,
            Self::Namespace(ns) => name.starts_with(ns) && name[ns.len()..].starts_with("::"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_capability_allows_read() {
        let cap = FilesystemCapability::ReadOnly(PathBuf::from("/data"));
        assert!(cap.allows_read(std::path::Path::new("/data/file.txt")));
        assert!(cap.allows_read(std::path::Path::new("/data")));
        assert!(!cap.allows_read(std::path::Path::new("/other")));
    }

    #[test]
    fn test_filesystem_capability_allows_write() {
        let read_only = FilesystemCapability::ReadOnly(PathBuf::from("/data"));
        assert!(!read_only.allows_write(std::path::Path::new("/data/file.txt")));

        let read_write = FilesystemCapability::ReadWrite(PathBuf::from("/data"));
        assert!(read_write.allows_write(std::path::Path::new("/data/file.txt")));
    }

    #[test]
    fn test_network_capability_allows_http_host() {
        let cap = NetworkCapability::HttpClient(vec![
            "api.example.com".to_string(),
            "*.trusted.com".to_string(),
        ]);

        assert!(cap.allows_http_host("api.example.com"));
        assert!(cap.allows_http_host("sub.trusted.com"));
        assert!(cap.allows_http_host("trusted.com"));
        assert!(!cap.allows_http_host("api.other.com"));
    }

    #[test]
    fn test_environment_capability_allows_var() {
        let specific = EnvironmentCapability::ReadVar("API_KEY".to_string());
        assert!(specific.allows_var("API_KEY"));
        assert!(!specific.allows_var("OTHER"));

        let all = EnvironmentCapability::ReadAll;
        assert!(all.allows_var("API_KEY"));
        assert!(all.allows_var("OTHER"));
    }

    #[test]
    fn test_host_function_capability() {
        let named = HostFunctionCapability::Named("log".to_string());
        assert!(named.allows_function("log"));
        assert!(!named.allows_function("other"));

        let ns = HostFunctionCapability::Namespace("db".to_string());
        assert!(ns.allows_function("db::query"));
        assert!(ns.allows_function("db::insert"));
        assert!(!ns.allows_function("cache::get"));
    }

    #[test]
    fn test_capability_display() {
        assert_eq!(Capability::stdout().to_string(), "stdio:stdout");
        assert_eq!(Capability::filesystem_read("/data").to_string(), "fs:read:/data");
        assert_eq!(
            Capability::http_client(vec!["api.example.com"]).to_string(),
            "net:http:api.example.com"
        );
    }
}
