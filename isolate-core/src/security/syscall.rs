//! System call definitions and filtering.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// System call information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Syscall {
    /// Syscall name.
    pub name: String,
    /// Syscall number (architecture-specific).
    pub number: i32,
    /// Number of arguments.
    pub num_args: u8,
    /// Brief description.
    pub description: Option<String>,
    /// Category.
    pub category: SyscallCategory,
}

/// System call category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyscallCategory {
    /// File I/O operations.
    FileIo,
    /// Memory management.
    Memory,
    /// Process control.
    Process,
    /// Signal handling.
    Signal,
    /// Network operations.
    Network,
    /// Time operations.
    Time,
    /// System information.
    System,
    /// IPC operations.
    Ipc,
    /// Security operations.
    Security,
    /// Other/miscellaneous.
    Other,
}

impl Syscall {
    /// Create a new syscall definition.
    pub fn new(name: impl Into<String>, number: i32, category: SyscallCategory) -> Self {
        Self { name: name.into(), number, num_args: 0, description: None, category }
    }

    /// Set the number of arguments.
    pub fn with_args(mut self, count: u8) -> Self {
        self.num_args = count;
        self
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Syscall argument for filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallArg {
    /// Argument index (0-5).
    pub index: u8,
    /// Argument value.
    pub value: u64,
}

impl SyscallArg {
    /// Create a new syscall argument.
    pub fn new(index: u8, value: u64) -> Self {
        Self { index, value }
    }
}

/// Syscall filter for runtime filtering.
#[derive(Debug, Clone)]
pub struct SyscallFilter {
    /// Allowed syscalls by number.
    allowed: HashMap<i32, Vec<ArgFilter>>,
    /// Denied syscalls by number.
    denied: HashMap<i32, Vec<ArgFilter>>,
    /// Default action (true = allow, false = deny).
    default_allow: bool,
    /// Syscall table for lookups.
    table: SyscallTable,
}

/// Argument filter condition.
#[derive(Debug, Clone)]
pub struct ArgFilter {
    /// Argument index.
    index: u8,
    /// Expected value.
    value: u64,
    /// Mask for comparison.
    mask: u64,
}

impl ArgFilter {
    /// Create a new exact match filter.
    pub fn exact(index: u8, value: u64) -> Self {
        Self { index, value, mask: u64::MAX }
    }

    /// Create a masked match filter.
    pub fn masked(index: u8, value: u64, mask: u64) -> Self {
        Self { index, value, mask }
    }

    /// Check if the argument matches.
    pub fn matches(&self, args: &[u64]) -> bool {
        if let Some(&arg) = args.get(self.index as usize) {
            (arg & self.mask) == (self.value & self.mask)
        } else {
            false
        }
    }
}

impl SyscallFilter {
    /// Create a new filter with default allow behavior.
    pub fn allow_all() -> Self {
        Self {
            allowed: HashMap::new(),
            denied: HashMap::new(),
            default_allow: true,
            table: SyscallTable::x86_64(),
        }
    }

    /// Create a new filter with default deny behavior.
    pub fn deny_all() -> Self {
        Self {
            allowed: HashMap::new(),
            denied: HashMap::new(),
            default_allow: false,
            table: SyscallTable::x86_64(),
        }
    }

    /// Allow a syscall.
    pub fn allow(&mut self, name: &str) {
        if let Some(num) = self.table.get_number(name) {
            self.allowed.insert(num, Vec::new());
        }
    }

    /// Allow a syscall with argument filter.
    pub fn allow_with_arg(&mut self, name: &str, filter: ArgFilter) {
        if let Some(num) = self.table.get_number(name) {
            self.allowed.entry(num).or_default().push(filter);
        }
    }

    /// Deny a syscall.
    pub fn deny(&mut self, name: &str) {
        if let Some(num) = self.table.get_number(name) {
            self.denied.insert(num, Vec::new());
        }
    }

    /// Deny a syscall with argument filter.
    pub fn deny_with_arg(&mut self, name: &str, filter: ArgFilter) {
        if let Some(num) = self.table.get_number(name) {
            self.denied.entry(num).or_default().push(filter);
        }
    }

    /// Check if a syscall should be allowed.
    pub fn check(&self, syscall_num: i32, args: &[u64]) -> bool {
        // Check denied first
        if let Some(filters) = self.denied.get(&syscall_num) {
            if filters.is_empty() {
                return false; // Blanket deny
            }
            if filters.iter().any(|f| f.matches(args)) {
                return false; // Matched deny filter
            }
        }

        // Check allowed
        if let Some(filters) = self.allowed.get(&syscall_num) {
            if filters.is_empty() {
                return true; // Blanket allow
            }
            if filters.iter().any(|f| f.matches(args)) {
                return true; // Matched allow filter
            }
            return false; // Has filters but none matched
        }

        self.default_allow
    }

    /// Get the syscall table.
    pub fn table(&self) -> &SyscallTable {
        &self.table
    }
}

/// Syscall number table.
#[derive(Debug, Clone)]
pub struct SyscallTable {
    /// Name to number mapping.
    by_name: HashMap<String, i32>,
    /// Number to name mapping.
    by_number: HashMap<i32, String>,
}

impl SyscallTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self { by_name: HashMap::new(), by_number: HashMap::new() }
    }

    /// Create x86_64 Linux syscall table.
    pub fn x86_64() -> Self {
        let mut table = Self::new();

        // Common x86_64 Linux syscalls
        let syscalls = [
            ("read", 0),
            ("write", 1),
            ("open", 2),
            ("close", 3),
            ("stat", 4),
            ("fstat", 5),
            ("lstat", 6),
            ("poll", 7),
            ("lseek", 8),
            ("mmap", 9),
            ("mprotect", 10),
            ("munmap", 11),
            ("brk", 12),
            ("rt_sigaction", 13),
            ("rt_sigprocmask", 14),
            ("rt_sigreturn", 15),
            ("ioctl", 16),
            ("pread64", 17),
            ("pwrite64", 18),
            ("readv", 19),
            ("writev", 20),
            ("access", 21),
            ("pipe", 22),
            ("select", 23),
            ("sched_yield", 24),
            ("mremap", 25),
            ("msync", 26),
            ("mincore", 27),
            ("madvise", 28),
            ("shmget", 29),
            ("shmat", 30),
            ("shmctl", 31),
            ("dup", 32),
            ("dup2", 33),
            ("pause", 34),
            ("nanosleep", 35),
            ("getitimer", 36),
            ("alarm", 37),
            ("setitimer", 38),
            ("getpid", 39),
            ("sendfile", 40),
            ("socket", 41),
            ("connect", 42),
            ("accept", 43),
            ("sendto", 44),
            ("recvfrom", 45),
            ("sendmsg", 46),
            ("recvmsg", 47),
            ("shutdown", 48),
            ("bind", 49),
            ("listen", 50),
            ("getsockname", 51),
            ("getpeername", 52),
            ("socketpair", 53),
            ("setsockopt", 54),
            ("getsockopt", 55),
            ("clone", 56),
            ("fork", 57),
            ("vfork", 58),
            ("execve", 59),
            ("exit", 60),
            ("wait4", 61),
            ("kill", 62),
            ("uname", 63),
            ("semget", 64),
            ("semop", 65),
            ("semctl", 66),
            ("shmdt", 67),
            ("msgget", 68),
            ("msgsnd", 69),
            ("msgrcv", 70),
            ("msgctl", 71),
            ("fcntl", 72),
            ("flock", 73),
            ("fsync", 74),
            ("fdatasync", 75),
            ("truncate", 76),
            ("ftruncate", 77),
            ("getdents", 78),
            ("getcwd", 79),
            ("chdir", 80),
            ("fchdir", 81),
            ("rename", 82),
            ("mkdir", 83),
            ("rmdir", 84),
            ("creat", 85),
            ("link", 86),
            ("unlink", 87),
            ("symlink", 88),
            ("readlink", 89),
            ("chmod", 90),
            ("fchmod", 91),
            ("chown", 92),
            ("fchown", 93),
            ("lchown", 94),
            ("umask", 95),
            ("gettimeofday", 96),
            ("getrlimit", 97),
            ("getrusage", 98),
            ("sysinfo", 99),
            ("times", 100),
            ("ptrace", 101),
            ("getuid", 102),
            ("syslog", 103),
            ("getgid", 104),
            ("setuid", 105),
            ("setgid", 106),
            ("geteuid", 107),
            ("getegid", 108),
            ("setpgid", 109),
            ("getppid", 110),
            ("getpgrp", 111),
            ("setsid", 112),
            ("setreuid", 113),
            ("setregid", 114),
            ("getgroups", 115),
            ("setgroups", 116),
            ("setresuid", 117),
            ("getresuid", 118),
            ("setresgid", 119),
            ("getresgid", 120),
            ("getpgid", 121),
            ("setfsuid", 122),
            ("setfsgid", 123),
            ("getsid", 124),
            ("capget", 125),
            ("capset", 126),
            ("rt_sigpending", 127),
            ("rt_sigtimedwait", 128),
            ("rt_sigqueueinfo", 129),
            ("rt_sigsuspend", 130),
            ("sigaltstack", 131),
            ("utime", 132),
            ("mknod", 133),
            ("uselib", 134),
            ("personality", 135),
            ("ustat", 136),
            ("statfs", 137),
            ("fstatfs", 138),
            ("sysfs", 139),
            ("getpriority", 140),
            ("setpriority", 141),
            ("sched_setparam", 142),
            ("sched_getparam", 143),
            ("sched_setscheduler", 144),
            ("sched_getscheduler", 145),
            ("sched_get_priority_max", 146),
            ("sched_get_priority_min", 147),
            ("sched_rr_get_interval", 148),
            ("mlock", 149),
            ("munlock", 150),
            ("mlockall", 151),
            ("munlockall", 152),
            ("vhangup", 153),
            ("modify_ldt", 154),
            ("pivot_root", 155),
            ("_sysctl", 156),
            ("prctl", 157),
            ("arch_prctl", 158),
            ("adjtimex", 159),
            ("setrlimit", 160),
            ("chroot", 161),
            ("sync", 162),
            ("acct", 163),
            ("settimeofday", 164),
            ("mount", 165),
            ("umount2", 166),
            ("swapon", 167),
            ("swapoff", 168),
            ("reboot", 169),
            ("sethostname", 170),
            ("setdomainname", 171),
            ("iopl", 172),
            ("ioperm", 173),
            ("create_module", 174),
            ("init_module", 175),
            ("delete_module", 176),
            ("get_kernel_syms", 177),
            ("query_module", 178),
            ("quotactl", 179),
            ("nfsservctl", 180),
            ("getpmsg", 181),
            ("putpmsg", 182),
            ("afs_syscall", 183),
            ("tuxcall", 184),
            ("security", 185),
            ("gettid", 186),
            ("readahead", 187),
            ("setxattr", 188),
            ("lsetxattr", 189),
            ("fsetxattr", 190),
            ("getxattr", 191),
            ("lgetxattr", 192),
            ("fgetxattr", 193),
            ("listxattr", 194),
            ("llistxattr", 195),
            ("flistxattr", 196),
            ("removexattr", 197),
            ("lremovexattr", 198),
            ("fremovexattr", 199),
            ("tkill", 200),
            ("time", 201),
            ("futex", 202),
            ("sched_setaffinity", 203),
            ("sched_getaffinity", 204),
            ("set_thread_area", 205),
            ("io_setup", 206),
            ("io_destroy", 207),
            ("io_getevents", 208),
            ("io_submit", 209),
            ("io_cancel", 210),
            ("get_thread_area", 211),
            ("lookup_dcookie", 212),
            ("epoll_create", 213),
            ("epoll_ctl_old", 214),
            ("epoll_wait_old", 215),
            ("remap_file_pages", 216),
            ("getdents64", 217),
            ("set_tid_address", 218),
            ("restart_syscall", 219),
            ("semtimedop", 220),
            ("fadvise64", 221),
            ("timer_create", 222),
            ("timer_settime", 223),
            ("timer_gettime", 224),
            ("timer_getoverrun", 225),
            ("timer_delete", 226),
            ("clock_settime", 227),
            ("clock_gettime", 228),
            ("clock_getres", 229),
            ("clock_nanosleep", 230),
            ("exit_group", 231),
            ("epoll_wait", 232),
            ("epoll_ctl", 233),
            ("tgkill", 234),
            ("utimes", 235),
            ("vserver", 236),
            ("mbind", 237),
            ("set_mempolicy", 238),
            ("get_mempolicy", 239),
            ("mq_open", 240),
            ("mq_unlink", 241),
            ("mq_timedsend", 242),
            ("mq_timedreceive", 243),
            ("mq_notify", 244),
            ("mq_getsetattr", 245),
            ("kexec_load", 246),
            ("waitid", 247),
            ("add_key", 248),
            ("request_key", 249),
            ("keyctl", 250),
            ("ioprio_set", 251),
            ("ioprio_get", 252),
            ("inotify_init", 253),
            ("inotify_add_watch", 254),
            ("inotify_rm_watch", 255),
            ("migrate_pages", 256),
            ("openat", 257),
            ("mkdirat", 258),
            ("mknodat", 259),
            ("fchownat", 260),
            ("futimesat", 261),
            ("newfstatat", 262),
            ("unlinkat", 263),
            ("renameat", 264),
            ("linkat", 265),
            ("symlinkat", 266),
            ("readlinkat", 267),
            ("fchmodat", 268),
            ("faccessat", 269),
            ("pselect6", 270),
            ("ppoll", 271),
            ("unshare", 272),
            ("set_robust_list", 273),
            ("get_robust_list", 274),
            ("splice", 275),
            ("tee", 276),
            ("sync_file_range", 277),
            ("vmsplice", 278),
            ("move_pages", 279),
            ("utimensat", 280),
            ("epoll_pwait", 281),
            ("signalfd", 282),
            ("timerfd_create", 283),
            ("eventfd", 284),
            ("fallocate", 285),
            ("timerfd_settime", 286),
            ("timerfd_gettime", 287),
            ("accept4", 288),
            ("signalfd4", 289),
            ("eventfd2", 290),
            ("epoll_create1", 291),
            ("dup3", 292),
            ("pipe2", 293),
            ("inotify_init1", 294),
            ("preadv", 295),
            ("pwritev", 296),
            ("rt_tgsigqueueinfo", 297),
            ("perf_event_open", 298),
            ("recvmmsg", 299),
            ("fanotify_init", 300),
            ("fanotify_mark", 301),
            ("prlimit64", 302),
            ("name_to_handle_at", 303),
            ("open_by_handle_at", 304),
            ("clock_adjtime", 305),
            ("syncfs", 306),
            ("sendmmsg", 307),
            ("setns", 308),
            ("getcpu", 309),
            ("process_vm_readv", 310),
            ("process_vm_writev", 311),
            ("kcmp", 312),
            ("finit_module", 313),
            ("sched_setattr", 314),
            ("sched_getattr", 315),
            ("renameat2", 316),
            ("seccomp", 317),
            ("getrandom", 318),
            ("memfd_create", 319),
            ("kexec_file_load", 320),
            ("bpf", 321),
            ("execveat", 322),
            ("userfaultfd", 323),
            ("membarrier", 324),
            ("mlock2", 325),
            ("copy_file_range", 326),
        ];

        for (name, num) in syscalls {
            table.add(name, num);
        }

        table
    }

    /// Add a syscall to the table.
    pub fn add(&mut self, name: impl Into<String>, number: i32) {
        let name = name.into();
        self.by_number.insert(number, name.clone());
        self.by_name.insert(name, number);
    }

    /// Get syscall number by name.
    pub fn get_number(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).copied()
    }

    /// Get syscall name by number.
    pub fn get_name(&self, number: i32) -> Option<&str> {
        self.by_number.get(&number).map(|s| s.as_str())
    }

    /// Get all syscall names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(|s| s.as_str())
    }

    /// Get all syscall numbers.
    pub fn numbers(&self) -> impl Iterator<Item = i32> + '_ {
        self.by_number.keys().copied()
    }

    /// Get the number of syscalls in the table.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

impl Default for SyscallTable {
    fn default() -> Self {
        Self::x86_64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_new() {
        let sc = Syscall::new("read", 0, SyscallCategory::FileIo)
            .with_args(3)
            .with_description("Read from file descriptor");

        assert_eq!(sc.name, "read");
        assert_eq!(sc.number, 0);
        assert_eq!(sc.num_args, 3);
        assert_eq!(sc.category, SyscallCategory::FileIo);
    }

    #[test]
    fn test_syscall_arg() {
        let arg = SyscallArg::new(0, 42);
        assert_eq!(arg.index, 0);
        assert_eq!(arg.value, 42);
    }

    #[test]
    fn test_arg_filter_exact() {
        let filter = ArgFilter::exact(0, 42);
        assert!(filter.matches(&[42, 0, 0]));
        assert!(!filter.matches(&[41, 0, 0]));
    }

    #[test]
    fn test_arg_filter_masked() {
        let filter = ArgFilter::masked(0, 0x80, 0xFF);
        assert!(filter.matches(&[0x80, 0, 0]));
        assert!(filter.matches(&[0x180, 0, 0])); // 0x180 & 0xFF == 0x80
        assert!(!filter.matches(&[0x81, 0, 0]));
    }

    #[test]
    fn test_syscall_filter_deny_all() {
        let filter = SyscallFilter::deny_all();
        assert!(!filter.check(0, &[])); // read
        assert!(!filter.check(1, &[])); // write
    }

    #[test]
    fn test_syscall_filter_allow_all() {
        let filter = SyscallFilter::allow_all();
        assert!(filter.check(0, &[])); // read
        assert!(filter.check(1, &[])); // write
    }

    #[test]
    fn test_syscall_filter_allow_specific() {
        let mut filter = SyscallFilter::deny_all();
        filter.allow("read");
        filter.allow("write");

        assert!(filter.check(0, &[])); // read
        assert!(filter.check(1, &[])); // write
        assert!(!filter.check(59, &[])); // execve
    }

    #[test]
    fn test_syscall_filter_deny_specific() {
        let mut filter = SyscallFilter::allow_all();
        filter.deny("execve");
        filter.deny("ptrace");

        assert!(filter.check(0, &[])); // read
        assert!(!filter.check(59, &[])); // execve
        assert!(!filter.check(101, &[])); // ptrace
    }

    #[test]
    fn test_syscall_filter_with_args() {
        let mut filter = SyscallFilter::deny_all();
        filter.allow_with_arg("ioctl", ArgFilter::exact(1, 0x5401));

        // Allowed with matching arg
        assert!(filter.check(16, &[3, 0x5401, 0]));
        // Denied with non-matching arg
        assert!(!filter.check(16, &[3, 0x5402, 0]));
    }

    #[test]
    fn test_syscall_table_x86_64() {
        let table = SyscallTable::x86_64();

        assert_eq!(table.get_number("read"), Some(0));
        assert_eq!(table.get_number("write"), Some(1));
        assert_eq!(table.get_number("execve"), Some(59));
        assert_eq!(table.get_number("exit"), Some(60));
        assert_eq!(table.get_number("getrandom"), Some(318));

        assert_eq!(table.get_name(0), Some("read"));
        assert_eq!(table.get_name(1), Some("write"));
    }

    #[test]
    fn test_syscall_table_custom() {
        let mut table = SyscallTable::new();
        table.add("custom", 999);

        assert_eq!(table.get_number("custom"), Some(999));
        assert_eq!(table.get_name(999), Some("custom"));
    }

    #[test]
    fn test_syscall_table_len() {
        let table = SyscallTable::x86_64();
        assert!(table.len() > 300); // x86_64 has ~350 syscalls
        assert!(!table.is_empty());
    }

    #[test]
    fn test_syscall_table_iteration() {
        let table = SyscallTable::x86_64();

        let names: Vec<_> = table.names().collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"write"));

        let numbers: Vec<_> = table.numbers().collect();
        assert!(numbers.contains(&0));
        assert!(numbers.contains(&1));
    }
}
