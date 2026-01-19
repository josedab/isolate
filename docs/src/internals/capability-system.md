# Capability System

Internal design of the capability-based security system.

## Design Principles

### Default Deny

Everything is forbidden unless explicitly allowed:

```rust
// Empty capability set - can't do anything
let caps = CapabilitySet::default();
assert!(!caps.has(&Capability::stdout()));
```

### Explicit Grants

Capabilities must be explicitly granted:

```rust
let mut caps = CapabilitySet::default();
caps.grant(Capability::stdout());
assert!(caps.has(&Capability::stdout()));
```

### Audit Trail

All capability checks are logged:

```rust
tracing::info!(
    sandbox_id = %self.sandbox_id,
    capability = ?cap,
    "capability_granted"
);
```

## Capability Types

### Definition

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    // Standard I/O
    Stdout,
    Stderr,
    Stdin,

    // Filesystem
    FilesystemRead(PathBuf),
    FilesystemWrite(PathBuf),

    // Network
    HttpClient(Vec<String>),
    TcpConnect(Vec<String>),
    DnsLookup,

    // System
    Clock,
    Random,

    // Environment
    EnvVar(String),
}
```

### Constructors

```rust
impl Capability {
    pub fn stdout() -> Self { Self::Stdout }
    pub fn stderr() -> Self { Self::Stderr }
    pub fn stdin() -> Self { Self::Stdin }
    pub fn clock() -> Self { Self::Clock }
    pub fn random() -> Self { Self::Random }

    pub fn filesystem_read(path: impl Into<PathBuf>) -> Self {
        Self::FilesystemRead(path.into())
    }

    pub fn filesystem_write(path: impl Into<PathBuf>) -> Self {
        Self::FilesystemWrite(path.into())
    }

    pub fn http_client(hosts: Vec<String>) -> Self {
        Self::HttpClient(hosts)
    }

    pub fn env_var(name: impl Into<String>) -> Self {
        Self::EnvVar(name.into())
    }
}
```

## CapabilitySet

### Storage

```rust
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    capabilities: HashSet<Capability>,
}
```

### Operations

```rust
impl CapabilitySet {
    pub fn grant(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    pub fn has(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter()
    }
}
```

## CapabilityEnforcer

### Structure

```rust
pub struct CapabilityEnforcer {
    capabilities: CapabilitySet,
    sandbox_id: Uuid,
}
```

### Check Methods

```rust
impl CapabilityEnforcer {
    pub fn check_stdout(&self) -> Result<()> {
        self.check(&Capability::Stdout)
    }

    pub fn check_stderr(&self) -> Result<()> {
        self.check(&Capability::Stderr)
    }

    pub fn check_fs_read(&self, path: &Path) -> Result<()> {
        // Check exact path
        if self.capabilities.has(&Capability::FilesystemRead(path.to_path_buf())) {
            return self.grant_access(Capability::FilesystemRead(path.to_path_buf()));
        }

        // Check parent directories
        for ancestor in path.ancestors().skip(1) {
            if self.capabilities.has(&Capability::FilesystemRead(ancestor.to_path_buf())) {
                return self.grant_access(Capability::FilesystemRead(path.to_path_buf()));
            }
        }

        self.deny_access(Capability::FilesystemRead(path.to_path_buf()))
    }

    pub fn check_http(&self, host: &str) -> Result<()> {
        for cap in self.capabilities.iter() {
            if let Capability::HttpClient(hosts) = cap {
                if hosts.iter().any(|h| h == host) {
                    return self.grant_access(Capability::HttpClient(vec![host.to_string()]));
                }
            }
        }

        self.deny_access(Capability::HttpClient(vec![host.to_string()]))
    }

    fn check(&self, cap: &Capability) -> Result<()> {
        if self.capabilities.has(cap) {
            self.grant_access(cap.clone())
        } else {
            self.deny_access(cap.clone())
        }
    }

    fn grant_access(&self, cap: Capability) -> Result<()> {
        tracing::debug!(
            sandbox_id = %self.sandbox_id,
            capability = ?cap,
            "capability_granted"
        );
        Ok(())
    }

    fn deny_access(&self, cap: Capability) -> Result<()> {
        tracing::warn!(
            sandbox_id = %self.sandbox_id,
            capability = ?cap,
            "capability_denied"
        );
        Err(Error::CapabilityDenied(cap))
    }
}
```

## Path Matching

### Hierarchical Paths

Filesystem capabilities use hierarchical matching:

```rust
// Grant /data
caps.grant(Capability::filesystem_read("/data"));

// These are allowed:
enforcer.check_fs_read(Path::new("/data"))?;           // exact
enforcer.check_fs_read(Path::new("/data/file.txt"))?;  // child
enforcer.check_fs_read(Path::new("/data/sub/file"))?;  // descendant

// These are denied:
enforcer.check_fs_read(Path::new("/data-other"))?;     // different path
enforcer.check_fs_read(Path::new("/"))?;               // parent
```

### Path Canonicalization

Paths are canonicalized to prevent traversal attacks:

```rust
pub fn check_fs_read(&self, path: &Path) -> Result<()> {
    // Canonicalize to resolve .. and symlinks
    let canonical = path.canonicalize()
        .map_err(|_| Error::FilesystemAccessDenied { path: path.to_path_buf() })?;

    // Check against canonical path
    self.check_fs_read_canonical(&canonical)
}
```

## Integration Points

### WASI Integration

Capabilities control WASI context setup:

```rust
// In WASI context builder
if enforcer.check_stdout().is_ok() {
    builder = builder.stdout(capture_stdout);
}

for cap in capabilities.iter() {
    if let Capability::FilesystemRead(path) = cap {
        builder = builder.preopened_dir(path, path, READ_PERMS)?;
    }
}
```

### Host Function Integration

Host functions check capabilities:

```rust
fn http_fetch(caller: Caller<'_, StoreData>, url_ptr: i32, url_len: i32) -> Result<i32> {
    let url = read_string_from_memory(&caller, url_ptr, url_len)?;
    let host = Url::parse(&url)?.host_str().unwrap_or("");

    // Check capability
    caller.data().enforcer.check_http(host)?;

    // Proceed with fetch
    // ...
}
```

## Audit Logging

### Log Format

```rust
// Granted
tracing::info!(
    target: "isolate::capability::audit",
    sandbox_id = %sandbox_id,
    capability = ?cap,
    event = "granted",
    "capability check passed"
);

// Denied
tracing::warn!(
    target: "isolate::capability::audit",
    sandbox_id = %sandbox_id,
    capability = ?cap,
    event = "denied",
    "capability check failed"
);
```

### Structured Output

```json
{
    "timestamp": "2024-01-15T10:30:00Z",
    "level": "WARN",
    "target": "isolate::capability::audit",
    "sandbox_id": "550e8400-e29b-41d4-a716-446655440000",
    "capability": {"FilesystemRead": "/etc/passwd"},
    "event": "denied"
}
```

## Security Considerations

### Time-of-Check Time-of-Use (TOCTOU)

Capabilities are checked at the WASI layer, close to the actual operation, minimizing TOCTOU windows.

### Capability Leakage

Capabilities are not exposed to WASM code. They're checked by the runtime, not the module.

### Delegation

Capabilities cannot be delegated. A sandbox cannot grant capabilities to other sandboxes.

## See Also

- [Security Model](../guide/security-model.md)
- [Architecture](./architecture.md)
