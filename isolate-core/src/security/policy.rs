//! Security policy definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Action to take when a syscall matches a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeccompAction {
    /// Allow the syscall.
    Allow,
    /// Return an error to the caller.
    Errno(i32),
    /// Terminate the process.
    Kill,
    /// Terminate the thread.
    KillThread,
    /// Log the syscall (but allow it).
    Log,
    /// Trace the syscall (for debugging).
    Trace(u32),
    /// Trap - sends SIGSYS.
    Trap,
}

impl Default for SeccompAction {
    fn default() -> Self {
        SeccompAction::Kill
    }
}

/// A seccomp rule matching a specific syscall pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompRule {
    /// Syscall number or name.
    pub syscall: String,
    /// Action to take when matched.
    pub action: SeccompAction,
    /// Optional argument conditions.
    pub arg_conditions: Vec<ArgCondition>,
    /// Whether this is an allow or deny rule.
    pub is_allow: bool,
    /// Priority (higher = evaluated first).
    pub priority: i32,
}

/// Condition on a syscall argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgCondition {
    /// Argument index (0-5).
    pub arg_index: u8,
    /// Comparison operation.
    pub op: CompareOp,
    /// Value to compare against.
    pub value: u64,
    /// Optional mask for bitwise comparisons.
    pub mask: Option<u64>,
}

/// Comparison operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Masked equal (value & mask == expected & mask).
    MaskedEq,
}

impl SeccompRule {
    /// Create a new allow rule for a syscall.
    pub fn allow(syscall: impl Into<String>) -> Self {
        Self {
            syscall: syscall.into(),
            action: SeccompAction::Allow,
            arg_conditions: Vec::new(),
            is_allow: true,
            priority: 0,
        }
    }

    /// Create a new deny rule for a syscall.
    pub fn deny(syscall: impl Into<String>) -> Self {
        Self {
            syscall: syscall.into(),
            action: SeccompAction::Kill,
            arg_conditions: Vec::new(),
            is_allow: false,
            priority: 0,
        }
    }

    /// Set the action.
    pub fn with_action(mut self, action: SeccompAction) -> Self {
        self.action = action;
        self
    }

    /// Add an argument condition.
    pub fn with_arg(mut self, index: u8, op: CompareOp, value: u64) -> Self {
        self.arg_conditions.push(ArgCondition {
            arg_index: index,
            op,
            value,
            mask: None,
        });
        self
    }

    /// Add a masked argument condition.
    pub fn with_arg_masked(mut self, index: u8, value: u64, mask: u64) -> Self {
        self.arg_conditions.push(ArgCondition {
            arg_index: index,
            op: CompareOp::MaskedEq,
            value,
            mask: Some(mask),
        });
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// Seccomp filtering policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccompPolicy {
    /// Default action for unmatched syscalls.
    pub default_action: SeccompAction,
    /// Syscall rules.
    pub rules: Vec<SeccompRule>,
    /// Architecture to apply the policy to.
    pub arch: Option<String>,
    /// Whether to log denied syscalls.
    pub log_denials: bool,
    /// Policy name for identification.
    pub name: Option<String>,
}

impl Default for SeccompPolicy {
    fn default() -> Self {
        Self::strict()
    }
}

impl SeccompPolicy {
    /// Create a new empty policy.
    pub fn new(default_action: SeccompAction) -> Self {
        Self {
            default_action,
            rules: Vec::new(),
            arch: None,
            log_denials: false,
            name: None,
        }
    }

    /// Create a strict policy that kills on any disallowed syscall.
    pub fn strict() -> Self {
        let mut policy = Self::new(SeccompAction::Kill);
        policy.name = Some("strict".to_string());

        // Allow only essential syscalls
        let essential = [
            "read",
            "write",
            "close",
            "fstat",
            "lseek",
            "mmap",
            "mprotect",
            "munmap",
            "brk",
            "rt_sigaction",
            "rt_sigprocmask",
            "rt_sigreturn",
            "ioctl",
            "exit",
            "exit_group",
            "arch_prctl",
            "futex",
            "clock_gettime",
            "getrandom",
            "sched_yield",
        ];

        for syscall in essential {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        policy
    }

    /// Create a permissive policy that allows most syscalls.
    pub fn permissive() -> Self {
        let mut policy = Self::new(SeccompAction::Allow);
        policy.name = Some("permissive".to_string());
        policy.log_denials = true;

        // Deny dangerous syscalls
        let dangerous = [
            "ptrace",
            "process_vm_readv",
            "process_vm_writev",
            "kexec_load",
            "kexec_file_load",
            "reboot",
            "sethostname",
            "setdomainname",
            "init_module",
            "finit_module",
            "delete_module",
            "acct",
            "swapon",
            "swapoff",
            "mount",
            "umount",
            "umount2",
            "pivot_root",
        ];

        for syscall in dangerous {
            policy.rules.push(SeccompRule::deny(syscall));
        }

        policy
    }

    /// Create a policy for sandboxed code execution.
    pub fn sandbox() -> Self {
        let mut policy = Self::new(SeccompAction::Errno(1)); // EPERM
        policy.name = Some("sandbox".to_string());
        policy.log_denials = true;

        // Allow file operations (limited)
        for syscall in [
            "read", "write", "close", "fstat", "lseek", "pread64", "pwrite64",
        ] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow memory operations
        for syscall in ["mmap", "mprotect", "munmap", "brk", "mremap"] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow signal handling
        for syscall in [
            "rt_sigaction",
            "rt_sigprocmask",
            "rt_sigreturn",
            "sigaltstack",
        ] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow time operations
        for syscall in ["clock_gettime", "clock_getres", "nanosleep"] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow thread/process control
        for syscall in ["exit", "exit_group", "sched_yield", "futex"] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow minimal system info
        for syscall in [
            "uname", "getpid", "gettid", "getuid", "getgid", "geteuid", "getegid",
        ] {
            policy.rules.push(SeccompRule::allow(syscall));
        }

        // Allow getrandom for crypto
        policy.rules.push(SeccompRule::allow("getrandom"));

        // Platform-specific
        policy.rules.push(SeccompRule::allow("arch_prctl"));
        policy.rules.push(SeccompRule::allow("prctl")); // Some prctl operations

        policy
    }

    /// Add a rule.
    pub fn add_rule(mut self, rule: SeccompRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Allow a syscall.
    pub fn allow(mut self, syscall: impl Into<String>) -> Self {
        self.rules.push(SeccompRule::allow(syscall));
        self
    }

    /// Deny a syscall.
    pub fn deny(mut self, syscall: impl Into<String>) -> Self {
        self.rules.push(SeccompRule::deny(syscall));
        self
    }

    /// Set logging of denials.
    pub fn with_logging(mut self, log: bool) -> Self {
        self.log_denials = log;
        self
    }

    /// Set policy name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Get allowed syscalls.
    pub fn allowed_syscalls(&self) -> HashSet<&str> {
        self.rules
            .iter()
            .filter(|r| r.is_allow)
            .map(|r| r.syscall.as_str())
            .collect()
    }

    /// Get denied syscalls.
    pub fn denied_syscalls(&self) -> HashSet<&str> {
        self.rules
            .iter()
            .filter(|r| !r.is_allow)
            .map(|r| r.syscall.as_str())
            .collect()
    }

    /// Check if a syscall would be allowed (simple check, no arg conditions).
    pub fn would_allow(&self, syscall: &str) -> bool {
        // Check rules in priority order
        let mut rules: Vec<_> = self.rules.iter().filter(|r| r.syscall == syscall).collect();
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        if let Some(rule) = rules.first() {
            rule.is_allow
        } else {
            matches!(self.default_action, SeccompAction::Allow)
        }
    }
}

/// Landlock filesystem access rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandlockAccess {
    /// Execute files.
    pub execute: bool,
    /// Write to files.
    pub write_file: bool,
    /// Read files.
    pub read_file: bool,
    /// Read directories.
    pub read_dir: bool,
    /// Remove directories.
    pub remove_dir: bool,
    /// Remove files.
    pub remove_file: bool,
    /// Create char devices.
    pub make_char: bool,
    /// Create directories.
    pub make_dir: bool,
    /// Create regular files.
    pub make_reg: bool,
    /// Create sockets.
    pub make_sock: bool,
    /// Create fifos.
    pub make_fifo: bool,
    /// Create block devices.
    pub make_block: bool,
    /// Create symlinks.
    pub make_sym: bool,
    /// Refer/link.
    pub refer: bool,
    /// Truncate files.
    pub truncate: bool,
}

impl Default for LandlockAccess {
    fn default() -> Self {
        Self::none()
    }
}

impl LandlockAccess {
    /// No access rights.
    pub fn none() -> Self {
        Self {
            execute: false,
            write_file: false,
            read_file: false,
            read_dir: false,
            remove_dir: false,
            remove_file: false,
            make_char: false,
            make_dir: false,
            make_reg: false,
            make_sock: false,
            make_fifo: false,
            make_block: false,
            make_sym: false,
            refer: false,
            truncate: false,
        }
    }

    /// Full access rights.
    pub fn full() -> Self {
        Self {
            execute: true,
            write_file: true,
            read_file: true,
            read_dir: true,
            remove_dir: true,
            remove_file: true,
            make_char: true,
            make_dir: true,
            make_reg: true,
            make_sock: true,
            make_fifo: true,
            make_block: true,
            make_sym: true,
            refer: true,
            truncate: true,
        }
    }

    /// Read-only access.
    pub fn read_only() -> Self {
        Self {
            read_file: true,
            read_dir: true,
            execute: true,
            ..Self::none()
        }
    }

    /// Read-write access.
    pub fn read_write() -> Self {
        Self {
            read_file: true,
            read_dir: true,
            write_file: true,
            truncate: true,
            make_reg: true,
            ..Self::none()
        }
    }
}

/// A landlock rule for a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockRule {
    /// Path to apply the rule to.
    pub path: PathBuf,
    /// Access rights for this path.
    pub access: LandlockAccess,
    /// Whether to apply to the path and its children.
    pub recursive: bool,
}

impl LandlockRule {
    /// Create a new rule for a path.
    pub fn new(path: impl Into<PathBuf>, access: LandlockAccess) -> Self {
        Self {
            path: path.into(),
            access,
            recursive: true,
        }
    }

    /// Create a read-only rule.
    pub fn read_only(path: impl Into<PathBuf>) -> Self {
        Self::new(path, LandlockAccess::read_only())
    }

    /// Create a read-write rule.
    pub fn read_write(path: impl Into<PathBuf>) -> Self {
        Self::new(path, LandlockAccess::read_write())
    }

    /// Create a full access rule.
    pub fn full_access(path: impl Into<PathBuf>) -> Self {
        Self::new(path, LandlockAccess::full())
    }

    /// Set whether the rule is recursive.
    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }
}

/// Landlock filesystem sandboxing policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandlockPolicy {
    /// Rules for specific paths.
    pub rules: Vec<LandlockRule>,
    /// Whether landlock is enabled.
    pub enabled: bool,
    /// Whether to allow file execution.
    pub allow_execute: bool,
    /// Policy name.
    pub name: Option<String>,
}

impl Default for LandlockPolicy {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            enabled: true,
            allow_execute: false,
            name: None,
        }
    }
}

impl LandlockPolicy {
    /// Create a new empty policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a policy with read-only access to specified paths.
    pub fn read_only(paths: &[impl AsRef<std::path::Path>]) -> Self {
        let mut policy = Self::new();
        policy.name = Some("read-only".to_string());

        for path in paths {
            policy.rules.push(LandlockRule::read_only(path.as_ref()));
        }

        policy
    }

    /// Create a policy with read-write access to specified paths.
    pub fn read_write(paths: &[impl AsRef<std::path::Path>]) -> Self {
        let mut policy = Self::new();
        policy.name = Some("read-write".to_string());

        for path in paths {
            policy.rules.push(LandlockRule::read_write(path.as_ref()));
        }

        policy
    }

    /// Add a rule.
    pub fn add_rule(mut self, rule: LandlockRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Allow read-only access to a path.
    pub fn allow_read(self, path: impl Into<PathBuf>) -> Self {
        self.add_rule(LandlockRule::read_only(path))
    }

    /// Allow read-write access to a path.
    pub fn allow_write(self, path: impl Into<PathBuf>) -> Self {
        self.add_rule(LandlockRule::read_write(path))
    }

    /// Allow full access to a path.
    pub fn allow_full(self, path: impl Into<PathBuf>) -> Self {
        self.add_rule(LandlockRule::full_access(path))
    }

    /// Enable or disable the policy.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set policy name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Get all paths with read access.
    pub fn readable_paths(&self) -> Vec<&PathBuf> {
        self.rules
            .iter()
            .filter(|r| r.access.read_file)
            .map(|r| &r.path)
            .collect()
    }

    /// Get all paths with write access.
    pub fn writable_paths(&self) -> Vec<&PathBuf> {
        self.rules
            .iter()
            .filter(|r| r.access.write_file)
            .map(|r| &r.path)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seccomp_action_default() {
        assert_eq!(SeccompAction::default(), SeccompAction::Kill);
    }

    #[test]
    fn test_seccomp_rule_allow() {
        let rule = SeccompRule::allow("read");
        assert!(rule.is_allow);
        assert_eq!(rule.action, SeccompAction::Allow);
    }

    #[test]
    fn test_seccomp_rule_deny() {
        let rule = SeccompRule::deny("ptrace");
        assert!(!rule.is_allow);
        assert_eq!(rule.action, SeccompAction::Kill);
    }

    #[test]
    fn test_seccomp_rule_with_arg() {
        let rule = SeccompRule::allow("ioctl").with_arg(1, CompareOp::Eq, 0x5401); // TCGETS

        assert_eq!(rule.arg_conditions.len(), 1);
        assert_eq!(rule.arg_conditions[0].arg_index, 1);
        assert_eq!(rule.arg_conditions[0].value, 0x5401);
    }

    #[test]
    fn test_seccomp_policy_strict() {
        let policy = SeccompPolicy::strict();

        assert_eq!(policy.default_action, SeccompAction::Kill);
        assert!(policy.would_allow("read"));
        assert!(policy.would_allow("write"));
        assert!(!policy.would_allow("ptrace"));
    }

    #[test]
    fn test_seccomp_policy_permissive() {
        let policy = SeccompPolicy::permissive();

        assert_eq!(policy.default_action, SeccompAction::Allow);
        assert!(policy.would_allow("read"));
        assert!(!policy.would_allow("ptrace"));
        assert!(!policy.would_allow("reboot"));
    }

    #[test]
    fn test_seccomp_policy_sandbox() {
        let policy = SeccompPolicy::sandbox();

        assert!(policy.would_allow("read"));
        assert!(policy.would_allow("write"));
        assert!(policy.would_allow("mmap"));
        assert!(!policy.would_allow("execve"));
    }

    #[test]
    fn test_seccomp_policy_builder() {
        let policy = SeccompPolicy::new(SeccompAction::Kill)
            .allow("read")
            .allow("write")
            .deny("execve")
            .with_logging(true)
            .with_name("custom");

        assert!(policy.would_allow("read"));
        assert!(!policy.would_allow("execve"));
        assert!(policy.log_denials);
        assert_eq!(policy.name, Some("custom".to_string()));
    }

    #[test]
    fn test_seccomp_policy_syscall_sets() {
        let policy = SeccompPolicy::sandbox();

        let allowed = policy.allowed_syscalls();
        assert!(allowed.contains("read"));
        assert!(allowed.contains("write"));
    }

    #[test]
    fn test_landlock_access_none() {
        let access = LandlockAccess::none();
        assert!(!access.read_file);
        assert!(!access.write_file);
        assert!(!access.execute);
    }

    #[test]
    fn test_landlock_access_full() {
        let access = LandlockAccess::full();
        assert!(access.read_file);
        assert!(access.write_file);
        assert!(access.execute);
        assert!(access.make_dir);
    }

    #[test]
    fn test_landlock_access_read_only() {
        let access = LandlockAccess::read_only();
        assert!(access.read_file);
        assert!(access.read_dir);
        assert!(!access.write_file);
    }

    #[test]
    fn test_landlock_rule() {
        let rule = LandlockRule::read_only("/lib");
        assert_eq!(rule.path, PathBuf::from("/lib"));
        assert!(rule.access.read_file);
        assert!(!rule.access.write_file);
    }

    #[test]
    fn test_landlock_policy_read_only() {
        let policy = LandlockPolicy::read_only(&["/lib", "/usr"]);
        assert_eq!(policy.rules.len(), 2);
        assert!(policy.readable_paths().contains(&&PathBuf::from("/lib")));
    }

    #[test]
    fn test_landlock_policy_builder() {
        let policy = LandlockPolicy::new()
            .allow_read("/lib")
            .allow_read("/usr")
            .allow_write("/tmp")
            .with_name("test");

        assert_eq!(policy.rules.len(), 3);
        assert!(policy.readable_paths().len() >= 2);
        assert!(policy.writable_paths().contains(&&PathBuf::from("/tmp")));
    }

    #[test]
    fn test_landlock_rule_recursive() {
        let rule = LandlockRule::full_access("/data").with_recursive(false);

        assert!(!rule.recursive);
    }
}
