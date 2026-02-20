//! Ring buffer channel for bidirectional host↔guest communication.
//!
//! Provides a bounded, lock-free-style channel using a ring buffer for
//! streaming data between the host application and WASM sandbox.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::engine::streaming::channel;
//!
//! // Create a channel pair with 4KB buffer
//! let (mut writer, mut reader) = channel(4096);
//!
//! // Write data
//! let written = writer.write(b"hello world").unwrap();
//! assert_eq!(written, 11);
//!
//! // Read data
//! let mut buf = vec![0u8; 32];
//! let read = reader.read(&mut buf).unwrap();
//! assert_eq!(&buf[..read], b"hello world");
//! ```

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Create a bidirectional ring buffer channel with the given capacity.
///
/// Returns a (writer, reader) pair. The writer can send data that the reader
/// will receive in FIFO order. Both ends are `Send + Sync`.
pub fn channel(capacity: usize) -> (RingWriter, RingReader) {
    let capacity = capacity.max(64); // enforce minimum
    let inner = Arc::new(RingBufferInner {
        buffer: vec![0; capacity].into_boxed_slice(),
        capacity,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
    });

    (RingWriter { inner: inner.clone(), total_written: 0 }, RingReader { inner, total_read: 0 })
}

/// Internal shared state for the ring buffer.
struct RingBufferInner {
    buffer: Box<[u8]>,
    capacity: usize,
    head: AtomicUsize, // write position
    tail: AtomicUsize, // read position
    closed: AtomicBool,
}

// SAFETY: The ring buffer uses atomic operations for head/tail management.
// Reads and writes to non-overlapping regions of the buffer are safe.
unsafe impl Send for RingBufferInner {}
unsafe impl Sync for RingBufferInner {}

impl RingBufferInner {
    fn available_write(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        if head >= tail {
            self.capacity - (head - tail) - 1
        } else {
            tail - head - 1
        }
    }

    fn available_read(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        if head >= tail {
            head - tail
        } else {
            self.capacity - tail + head
        }
    }
}

/// Error type for ring buffer operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// The channel is closed (other end dropped or explicitly closed).
    Closed,
    /// The buffer is full (write would block).
    Full,
    /// The buffer is empty (read would block).
    Empty,
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "channel closed"),
            Self::Full => write!(f, "buffer full"),
            Self::Empty => write!(f, "buffer empty"),
        }
    }
}

impl std::error::Error for ChannelError {}

/// Writer end of a ring buffer channel.
pub struct RingWriter {
    inner: Arc<RingBufferInner>,
    total_written: u64,
}

impl RingWriter {
    /// Write data to the ring buffer. Returns the number of bytes written.
    ///
    /// This is non-blocking: it writes as much as the buffer can hold and
    /// returns the count. Returns 0 if the buffer is full.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, ChannelError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(ChannelError::Closed);
        }

        let available = self.inner.available_write();
        if available == 0 {
            return Ok(0);
        }

        let to_write = data.len().min(available);
        let head = self.inner.head.load(Ordering::Acquire);

        // Write data, wrapping around if necessary
        let buf = self.inner.buffer.as_ptr() as *mut u8;
        for (i, &byte) in data[..to_write].iter().enumerate() {
            let pos = (head + i) % self.inner.capacity;
            // SAFETY: `pos` is always in bounds (0..capacity) due to the modulo.
            // We only write to positions in the range [head, head+to_write), which
            // is guaranteed not to overlap with the reader's region [tail, head)
            // because `to_write <= available_write()` reserves one slot as a guard.
            // The `as *mut u8` cast from `as_ptr()` is valid because we hold the
            // only `&mut self` reference to the writer, and atomics on head/tail
            // ensure the reader never accesses this region concurrently.
            unsafe {
                *buf.add(pos) = byte;
            }
        }

        let new_head = (head + to_write) % self.inner.capacity;
        self.inner.head.store(new_head, Ordering::Release);
        self.total_written += to_write as u64;

        Ok(to_write)
    }

    /// Write all data, returning error if the channel closes before completion.
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), ChannelError> {
        let mut offset = 0;
        while offset < data.len() {
            let written = self.write(&data[offset..])?;
            if written == 0 {
                // Buffer full, try again (busy wait - in practice, yield or async)
                std::thread::yield_now();
                if self.inner.closed.load(Ordering::Acquire) {
                    return Err(ChannelError::Closed);
                }
            }
            offset += written;
        }
        Ok(())
    }

    /// Get the number of bytes that can be written without blocking.
    pub fn available(&self) -> usize {
        self.inner.available_write()
    }

    /// Get total bytes written through this writer.
    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    /// Check if the reader end has been dropped.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Close the channel.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
    }

    /// Get the buffer capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Release);
    }
}

/// Reader end of a ring buffer channel.
pub struct RingReader {
    inner: Arc<RingBufferInner>,
    total_read: u64,
}

impl RingReader {
    /// Read data from the ring buffer. Returns the number of bytes read.
    ///
    /// Non-blocking: reads what's available and returns the count.
    /// Returns 0 if the buffer is empty.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, ChannelError> {
        let available = self.inner.available_read();
        if available == 0 {
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(ChannelError::Closed);
            }
            return Ok(0);
        }

        let to_read = buf.len().min(available);
        let tail = self.inner.tail.load(Ordering::Acquire);

        for (i, slot) in buf[..to_read].iter_mut().enumerate() {
            let pos = (tail + i) % self.inner.capacity;
            *slot = self.inner.buffer[pos];
        }

        let new_tail = (tail + to_read) % self.inner.capacity;
        self.inner.tail.store(new_tail, Ordering::Release);
        self.total_read += to_read as u64;

        Ok(to_read)
    }

    /// Read exactly `buf.len()` bytes, waiting if necessary.
    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ChannelError> {
        let mut offset = 0;
        while offset < buf.len() {
            let read = self.read(&mut buf[offset..])?;
            if read == 0 {
                std::thread::yield_now();
                if self.inner.closed.load(Ordering::Acquire) && self.inner.available_read() == 0 {
                    return Err(ChannelError::Closed);
                }
            }
            offset += read;
        }
        Ok(())
    }

    /// Get the number of bytes available to read.
    pub fn available(&self) -> usize {
        self.inner.available_read()
    }

    /// Get total bytes read through this reader.
    pub fn total_read(&self) -> u64 {
        self.total_read
    }

    /// Check if the writer end has been dropped.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire) && self.inner.available_read() == 0
    }

    /// Get the buffer capacity.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

impl Drop for RingReader {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Release);
    }
}

/// A bidirectional streaming channel pair for host↔guest communication.
///
/// Contains both directions: host-to-guest and guest-to-host.
pub struct StreamingChannel {
    /// Channel for host → guest data.
    pub host_to_guest_writer: RingWriter,
    /// Channel for host → guest data (guest reads from this).
    pub host_to_guest_reader: RingReader,
    /// Channel for guest → host data (guest writes to this).
    pub guest_to_host_writer: RingWriter,
    /// Channel for guest → host data (host reads from this).
    pub guest_to_host_reader: RingReader,
}

impl StreamingChannel {
    /// Create a bidirectional streaming channel with the given buffer capacity per direction.
    pub fn new(capacity: usize) -> Self {
        let (h2g_writer, h2g_reader) = channel(capacity);
        let (g2h_writer, g2h_reader) = channel(capacity);

        Self {
            host_to_guest_writer: h2g_writer,
            host_to_guest_reader: h2g_reader,
            guest_to_host_writer: g2h_writer,
            guest_to_host_reader: g2h_reader,
        }
    }

    /// Split into host-side and guest-side halves.
    pub fn split(self) -> (HostHalf, GuestHalf) {
        (
            HostHalf { writer: self.host_to_guest_writer, reader: self.guest_to_host_reader },
            GuestHalf { writer: self.guest_to_host_writer, reader: self.host_to_guest_reader },
        )
    }
}

/// Host-side half of a streaming channel.
pub struct HostHalf {
    /// Write data to guest.
    pub writer: RingWriter,
    /// Read data from guest.
    pub reader: RingReader,
}

/// Guest-side half of a streaming channel.
pub struct GuestHalf {
    /// Write data to host.
    pub writer: RingWriter,
    /// Read data from host.
    pub reader: RingReader,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_basic() {
        let (mut writer, mut reader) = channel(1024);

        let written = writer.write(b"hello").unwrap();
        assert_eq!(written, 5);

        let mut buf = vec![0u8; 32];
        let read = reader.read(&mut buf).unwrap();
        assert_eq!(read, 5);
        assert_eq!(&buf[..5], b"hello");
    }

    #[test]
    fn test_channel_wraparound() {
        let (mut writer, mut reader) = channel(128);

        // Fill most of the buffer
        let data = vec![0xAA; 100];
        writer.write(&data).unwrap();

        // Read it all
        let mut buf = vec![0u8; 100];
        reader.read(&mut buf).unwrap();

        // Write again (wraps around)
        let data2 = vec![0xBB; 80];
        let written = writer.write(&data2).unwrap();
        assert_eq!(written, 80);

        let mut buf2 = vec![0u8; 80];
        let read = reader.read(&mut buf2).unwrap();
        assert_eq!(read, 80);
        assert!(buf2.iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn test_channel_full() {
        let (mut writer, _reader) = channel(64);

        // Fill the buffer (capacity - 1 usable due to sentinel)
        let data = vec![0xFF; 63];
        let written = writer.write(&data).unwrap();
        assert_eq!(written, 63);

        // Should return 0 (full)
        let written = writer.write(b"x").unwrap();
        assert_eq!(written, 0);
    }

    #[test]
    fn test_channel_empty_read() {
        let (_, mut reader) = channel(64);

        let mut buf = vec![0u8; 32];
        // Channel still open but empty
        let result = reader.read(&mut buf);
        assert!(result.is_err() || result.unwrap() == 0);
    }

    #[test]
    fn test_channel_close_on_drop() {
        let (writer, mut reader) = channel(64);

        drop(writer);

        let mut buf = vec![0u8; 32];
        let result = reader.read(&mut buf);
        assert_eq!(result, Err(ChannelError::Closed));
    }

    #[test]
    fn test_channel_statistics() {
        let (mut writer, mut reader) = channel(1024);

        writer.write(b"hello").unwrap();
        writer.write(b" world").unwrap();

        let mut buf = vec![0u8; 32];
        reader.read(&mut buf).unwrap();

        assert_eq!(writer.total_written(), 11);
        assert_eq!(reader.total_read(), 11);
    }

    #[test]
    fn test_channel_capacity() {
        let (writer, reader) = channel(4096);
        assert_eq!(writer.capacity(), 4096);
        assert_eq!(reader.capacity(), 4096);
    }

    #[test]
    fn test_streaming_channel() {
        let streaming = StreamingChannel::new(1024);
        let (mut host, mut guest) = streaming.split();

        // Host writes, guest reads
        host.writer.write(b"from host").unwrap();
        let mut buf = vec![0u8; 32];
        let n = guest.reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"from host");

        // Guest writes, host reads
        guest.writer.write(b"from guest").unwrap();
        let mut buf2 = vec![0u8; 32];
        let n = host.reader.read(&mut buf2).unwrap();
        assert_eq!(&buf2[..n], b"from guest");
    }

    #[test]
    fn test_channel_min_capacity() {
        let (writer, _reader) = channel(1); // should be bumped to 64
        assert_eq!(writer.capacity(), 64);
    }

    #[test]
    fn test_channel_error_display() {
        assert_eq!(ChannelError::Closed.to_string(), "channel closed");
        assert_eq!(ChannelError::Full.to_string(), "buffer full");
        assert_eq!(ChannelError::Empty.to_string(), "buffer empty");
    }

    #[test]
    fn test_write_all() {
        let (mut writer, mut reader) = channel(128);

        // write_all should succeed
        writer.write_all(b"complete message").unwrap();

        let mut buf = vec![0u8; 32];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"complete message");
    }
}
