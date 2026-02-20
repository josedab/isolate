//! WASI Preview2 host implementations.

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// State for tracking host function calls and resource usage.
#[derive(Debug)]
pub struct WasiHostState {
    /// Start time for duration tracking.
    start_time: Instant,
    /// Total bytes read.
    bytes_read: Arc<RwLock<u64>>,
    /// Total bytes written.
    bytes_written: Arc<RwLock<u64>>,
    /// Number of filesystem operations.
    fs_operations: Arc<RwLock<u64>>,
    /// Number of network operations.
    net_operations: Arc<RwLock<u64>>,
    /// Whether the sandbox has been terminated.
    terminated: Arc<RwLock<bool>>,
}

impl WasiHostState {
    /// Create a new host state.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            bytes_read: Arc::new(RwLock::new(0)),
            bytes_written: Arc::new(RwLock::new(0)),
            fs_operations: Arc::new(RwLock::new(0)),
            net_operations: Arc::new(RwLock::new(0)),
            terminated: Arc::new(RwLock::new(false)),
        }
    }

    /// Get elapsed time since creation.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Record bytes read.
    pub fn record_read(&self, bytes: u64) {
        *self.bytes_read.write() += bytes;
    }

    /// Record bytes written.
    pub fn record_write(&self, bytes: u64) {
        *self.bytes_written.write() += bytes;
    }

    /// Record a filesystem operation.
    pub fn record_fs_op(&self) {
        *self.fs_operations.write() += 1;
    }

    /// Record a network operation.
    pub fn record_net_op(&self) {
        *self.net_operations.write() += 1;
    }

    /// Get total bytes read.
    pub fn bytes_read(&self) -> u64 {
        *self.bytes_read.read()
    }

    /// Get total bytes written.
    pub fn bytes_written(&self) -> u64 {
        *self.bytes_written.read()
    }

    /// Get filesystem operation count.
    pub fn fs_operation_count(&self) -> u64 {
        *self.fs_operations.read()
    }

    /// Get network operation count.
    pub fn net_operation_count(&self) -> u64 {
        *self.net_operations.read()
    }

    /// Mark as terminated.
    pub fn terminate(&self) {
        *self.terminated.write() = true;
    }

    /// Check if terminated.
    pub fn is_terminated(&self) -> bool {
        *self.terminated.read()
    }
}

impl Default for WasiHostState {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for I/O limits.
#[derive(Debug, Clone)]
pub struct IoLimits {
    /// Maximum bytes to read.
    pub max_read_bytes: Option<u64>,
    /// Maximum bytes to write.
    pub max_write_bytes: Option<u64>,
    /// Maximum filesystem operations.
    pub max_fs_operations: Option<u64>,
    /// Maximum network operations.
    pub max_net_operations: Option<u64>,
}

impl Default for IoLimits {
    fn default() -> Self {
        Self {
            max_read_bytes: None,
            max_write_bytes: None,
            max_fs_operations: None,
            max_net_operations: None,
        }
    }
}

impl IoLimits {
    /// Create unlimited I/O limits.
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Create with specific read/write limits.
    pub fn with_bytes(read: u64, write: u64) -> Self {
        Self { max_read_bytes: Some(read), max_write_bytes: Some(write), ..Default::default() }
    }
}

/// WASI clock ID constants.
#[allow(dead_code)]
pub mod clock {
    /// Real-time clock (wall clock time).
    pub const REALTIME: u32 = 0;
    /// Monotonic clock (for measuring durations).
    pub const MONOTONIC: u32 = 1;
    /// Process CPU time clock.
    pub const PROCESS_CPUTIME_ID: u32 = 2;
    /// Thread CPU time clock.
    pub const THREAD_CPUTIME_ID: u32 = 3;
}

/// WASI error codes for preview2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WasiError {
    /// Operation succeeded.
    Success = 0,
    /// Argument list too long.
    TooBig = 1,
    /// Permission denied.
    Access = 2,
    /// Address in use.
    AddressInUse = 3,
    /// Address not available.
    AddressNotAvailable = 4,
    /// Address family not supported.
    AddressFamilyNotSupported = 5,
    /// Resource unavailable, try again.
    Again = 6,
    /// Connection already in progress.
    Already = 7,
    /// Bad file descriptor.
    BadDescriptor = 8,
    /// Device or resource busy.
    Busy = 9,
    /// Connection aborted.
    ConnectionAborted = 10,
    /// Connection refused.
    ConnectionRefused = 11,
    /// Connection reset.
    ConnectionReset = 12,
    /// Resource deadlock would occur.
    Deadlock = 13,
    /// Destination address required.
    DestinationAddressRequired = 14,
    /// Mathematics argument out of domain.
    Domain = 15,
    /// File exists.
    Exist = 16,
    /// Bad address.
    Fault = 17,
    /// File too large.
    FileTooBig = 18,
    /// Host is unreachable.
    HostUnreachable = 19,
    /// Identifier removed.
    IdentifierRemoved = 20,
    /// Illegal byte sequence.
    IllegalByteSequence = 21,
    /// Operation in progress.
    InProgress = 22,
    /// Interrupted function.
    Interrupted = 23,
    /// Invalid argument.
    Invalid = 24,
    /// I/O error.
    Io = 25,
    /// Socket is connected.
    IsConnected = 26,
    /// Is a directory.
    IsDirectory = 27,
    /// Too many levels of symbolic links.
    Loop = 28,
    /// File descriptor value too large.
    TooManyLinks = 29,
    /// Message too large.
    MessageSize = 30,
    /// Filename too long.
    NameTooLong = 31,
    /// Network is down.
    NetworkDown = 32,
    /// Connection aborted by network.
    NetworkReset = 33,
    /// Network unreachable.
    NetworkUnreachable = 34,
    /// Too many files open in system.
    TooManyOpenFilesInSystem = 35,
    /// No buffer space available.
    NoBufferSpace = 36,
    /// No such device.
    NoDevice = 37,
    /// No such file or directory.
    NoEntry = 38,
    /// Executable file format error.
    NoExec = 39,
    /// No locks available.
    NoLock = 40,
    /// Not enough space.
    InsufficientMemory = 41,
    /// No message of desired type.
    NoMessage = 42,
    /// Protocol not available.
    NoProtocolOption = 43,
    /// No space left on device.
    NoSpace = 44,
    /// Function not supported.
    Unsupported = 45,
    /// Socket not connected.
    NotConnected = 46,
    /// Not a directory.
    NotDirectory = 47,
    /// Directory not empty.
    NotEmpty = 48,
    /// State not recoverable.
    NotRecoverable = 49,
    /// Not a socket.
    NotSocket = 50,
    /// Not supported.
    NotSupported = 51,
    /// Inappropriate I/O control operation.
    NoTty = 52,
    /// No such device or address.
    NoSuchDevice = 53,
    /// Value too large for data type.
    Overflow = 54,
    /// Previous owner died.
    OwnerDead = 55,
    /// Operation not permitted.
    NotPermitted = 56,
    /// Broken pipe.
    Pipe = 57,
    /// Protocol error.
    Protocol = 58,
    /// Protocol not supported.
    ProtocolNotSupported = 59,
    /// Protocol wrong type for socket.
    ProtocolType = 60,
    /// Result too large.
    Range = 61,
    /// Read-only file system.
    ReadOnly = 62,
    /// Invalid seek.
    Seek = 63,
    /// No such process.
    Search = 64,
    /// Connection timed out.
    TimedOut = 65,
    /// Text file busy.
    TextBusy = 66,
    /// Cross-device link.
    CrossDevice = 67,
}

impl WasiError {
    /// Convert from a standard I/O error.
    pub fn from_io_error(err: &std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match err.kind() {
            NotFound => Self::NoEntry,
            PermissionDenied => Self::Access,
            ConnectionRefused => Self::ConnectionRefused,
            ConnectionReset => Self::ConnectionReset,
            ConnectionAborted => Self::ConnectionAborted,
            NotConnected => Self::NotConnected,
            AddrInUse => Self::AddressInUse,
            AddrNotAvailable => Self::AddressNotAvailable,
            BrokenPipe => Self::Pipe,
            AlreadyExists => Self::Exist,
            WouldBlock => Self::Again,
            InvalidInput | InvalidData => Self::Invalid,
            TimedOut => Self::TimedOut,
            Interrupted => Self::Interrupted,
            WriteZero | UnexpectedEof => Self::Io,
            _ => Self::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_state_creation() {
        let state = WasiHostState::new();
        assert_eq!(state.bytes_read(), 0);
        assert_eq!(state.bytes_written(), 0);
        assert!(!state.is_terminated());
    }

    #[test]
    fn test_host_state_tracking() {
        let state = WasiHostState::new();

        state.record_read(100);
        state.record_write(200);
        state.record_fs_op();
        state.record_net_op();

        assert_eq!(state.bytes_read(), 100);
        assert_eq!(state.bytes_written(), 200);
        assert_eq!(state.fs_operation_count(), 1);
        assert_eq!(state.net_operation_count(), 1);
    }

    #[test]
    fn test_host_state_termination() {
        let state = WasiHostState::new();
        assert!(!state.is_terminated());

        state.terminate();
        assert!(state.is_terminated());
    }

    #[test]
    fn test_io_limits() {
        let limits = IoLimits::unlimited();
        assert!(limits.max_read_bytes.is_none());

        let limits = IoLimits::with_bytes(1024, 2048);
        assert_eq!(limits.max_read_bytes, Some(1024));
        assert_eq!(limits.max_write_bytes, Some(2048));
    }

    #[test]
    fn test_wasi_error_from_io() {
        use std::io::{Error, ErrorKind};

        let err = Error::new(ErrorKind::NotFound, "not found");
        assert_eq!(WasiError::from_io_error(&err), WasiError::NoEntry);

        let err = Error::new(ErrorKind::PermissionDenied, "denied");
        assert_eq!(WasiError::from_io_error(&err), WasiError::Access);
    }
}
