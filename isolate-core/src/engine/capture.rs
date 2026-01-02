//! I/O streams for WASI stdin/stdout/stderr.
//!
//! This module provides stream implementations for:
//! - Capturing stdout/stderr to in-memory buffers with optional I/O metering
//! - Providing input data to stdin with optional I/O metering
//! - Null streams that discard output

use crate::resource::ResourceMeter;
use bytes::Bytes;
use parking_lot::RwLock;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::Arc;
use wasmtime_wasi::{
    HostInputStream, HostOutputStream, StdinStream, StdoutStream, StreamResult, Subscribe,
};

/// A buffer that captures output written to it.
pub type CaptureBuffer = Arc<RwLock<Vec<u8>>>;

/// Creates a new capture buffer.
pub fn new_capture_buffer() -> CaptureBuffer {
    Arc::new(RwLock::new(Vec::new()))
}

/// A stdout/stderr stream that captures output to an in-memory buffer.
///
/// This implements `StdoutStream` and can be used with `WasiCtxBuilder::stdout()`
/// or `WasiCtxBuilder::stderr()` to capture WASM output.
#[derive(Clone)]
pub struct CaptureStream {
    buffer: CaptureBuffer,
    meter: Option<ResourceMeter>,
}

impl CaptureStream {
    /// Create a new capture stream with the given buffer.
    pub fn new(buffer: CaptureBuffer) -> Self {
        Self {
            buffer,
            meter: None,
        }
    }

    /// Create a new capture stream with metering.
    pub fn with_meter(buffer: CaptureBuffer, meter: ResourceMeter) -> Self {
        Self {
            buffer,
            meter: Some(meter),
        }
    }

    /// Get the captured bytes.
    pub fn contents(&self) -> Vec<u8> {
        self.buffer.read().clone()
    }
}

impl StdoutStream for CaptureStream {
    fn stream(&self) -> Box<dyn HostOutputStream> {
        Box::new(CaptureOutputStream::new(
            self.buffer.clone(),
            self.meter.clone(),
        ))
    }

    fn isatty(&self) -> bool {
        false
    }
}

/// The output stream implementation that writes to the capture buffer.
struct CaptureOutputStream {
    buffer: CaptureBuffer,
    meter: Option<ResourceMeter>,
}

impl CaptureOutputStream {
    fn new(buffer: CaptureBuffer, meter: Option<ResourceMeter>) -> Self {
        Self { buffer, meter }
    }
}

impl HostOutputStream for CaptureOutputStream {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        // Check I/O limits if metering is enabled
        if let Some(ref meter) = self.meter {
            if meter.record_write(bytes.len() as u64).is_err() {
                return Err(wasmtime_wasi::StreamError::Closed);
            }
        }
        self.buffer.write().extend_from_slice(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        // No buffering, writes go directly to the buffer
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        // Always ready to accept writes
        Ok(usize::MAX)
    }
}

impl Subscribe for CaptureOutputStream {
    fn ready<'a, 'b>(&'a mut self) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'b>>
    where
        Self: 'b,
        'a: 'b,
    {
        // Always ready immediately
        Box::pin(std::future::ready(()))
    }
}

/// A null output stream that discards all output.
///
/// Used when stdout/stderr capability is not granted.
#[derive(Clone)]
pub struct NullStream;

impl StdoutStream for NullStream {
    fn stream(&self) -> Box<dyn HostOutputStream> {
        Box::new(NullOutputStream)
    }

    fn isatty(&self) -> bool {
        false
    }
}

struct NullOutputStream;

impl HostOutputStream for NullOutputStream {
    fn write(&mut self, _bytes: Bytes) -> StreamResult<()> {
        // Discard all output
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(usize::MAX)
    }
}

impl Subscribe for NullOutputStream {
    fn ready<'a, 'b>(&'a mut self) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'b>>
    where
        Self: 'b,
        'a: 'b,
    {
        Box::pin(std::future::ready(()))
    }
}

/// A buffer for providing input data to stdin.
pub type InputBuffer = Arc<RwLock<Cursor<Vec<u8>>>>;

/// Creates a new input buffer with the given data.
pub fn new_input_buffer(data: Vec<u8>) -> InputBuffer {
    Arc::new(RwLock::new(Cursor::new(data)))
}

/// A stdin stream that provides data from an in-memory buffer.
///
/// This implements `StdinStream` and can be used with `WasiCtxBuilder::stdin()`
/// to provide input to WASM modules.
#[derive(Clone)]
pub struct BufferedStdin {
    buffer: InputBuffer,
    meter: Option<ResourceMeter>,
}

impl BufferedStdin {
    /// Create a new buffered stdin with the given data.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            buffer: new_input_buffer(data),
            meter: None,
        }
    }

    /// Create a new buffered stdin with metering.
    pub fn with_meter(data: Vec<u8>, meter: ResourceMeter) -> Self {
        Self {
            buffer: new_input_buffer(data),
            meter: Some(meter),
        }
    }
}

impl StdinStream for BufferedStdin {
    fn stream(&self) -> Box<dyn HostInputStream> {
        Box::new(BufferedInputStream::new(
            self.buffer.clone(),
            self.meter.clone(),
        ))
    }

    fn isatty(&self) -> bool {
        false
    }
}

/// The input stream implementation that reads from the input buffer.
struct BufferedInputStream {
    buffer: InputBuffer,
    meter: Option<ResourceMeter>,
}

impl BufferedInputStream {
    fn new(buffer: InputBuffer, meter: Option<ResourceMeter>) -> Self {
        Self { buffer, meter }
    }
}

impl HostInputStream for BufferedInputStream {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        use std::io::Read;
        let mut guard = self.buffer.write();
        let mut buf = vec![0u8; size];
        let n = guard.read(&mut buf).unwrap_or(0);
        buf.truncate(n);

        // Check I/O limits if metering is enabled
        if n > 0 {
            if let Some(ref meter) = self.meter {
                if meter.record_read(n as u64).is_err() {
                    return Err(wasmtime_wasi::StreamError::Closed);
                }
            }
        }

        Ok(Bytes::from(buf))
    }
}

impl Subscribe for BufferedInputStream {
    fn ready<'a, 'b>(&'a mut self) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'b>>
    where
        Self: 'b,
        'a: 'b,
    {
        // Always ready immediately
        Box::pin(std::future::ready(()))
    }
}

/// An empty stdin stream that returns EOF immediately.
///
/// Used when stdin capability is not granted or no input is provided.
#[derive(Clone)]
pub struct EmptyStdin;

impl StdinStream for EmptyStdin {
    fn stream(&self) -> Box<dyn HostInputStream> {
        Box::new(EmptyInputStream)
    }

    fn isatty(&self) -> bool {
        false
    }
}

struct EmptyInputStream;

impl HostInputStream for EmptyInputStream {
    fn read(&mut self, _size: usize) -> StreamResult<Bytes> {
        // Return empty bytes (EOF)
        Ok(Bytes::new())
    }
}

impl Subscribe for EmptyInputStream {
    fn ready<'a, 'b>(&'a mut self) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'b>>
    where
        Self: 'b,
        'a: 'b,
    {
        Box::pin(std::future::ready(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_stream_write() {
        let buffer = new_capture_buffer();
        let stream = CaptureStream::new(buffer.clone());

        // Get an output stream and write to it
        let mut output = stream.stream();
        output.write(Bytes::from("hello")).unwrap();
        output.write(Bytes::from(" world")).unwrap();

        // Check the captured content
        assert_eq!(stream.contents(), b"hello world");
    }

    #[test]
    fn test_capture_stream_multiple_streams() {
        let buffer = new_capture_buffer();
        let stream = CaptureStream::new(buffer.clone());

        // Multiple streams write to the same buffer
        let mut output1 = stream.stream();
        let mut output2 = stream.stream();

        output1.write(Bytes::from("first")).unwrap();
        output2.write(Bytes::from("second")).unwrap();

        // Both writes should be captured
        let contents = stream.contents();
        assert!(contents.starts_with(b"first") || contents.starts_with(b"second"));
        assert_eq!(contents.len(), 11); // "first" + "second"
    }

    #[test]
    fn test_null_stream() {
        let stream = NullStream;
        let mut output = stream.stream();

        // Writes should succeed but be discarded
        assert!(output.write(Bytes::from("discarded")).is_ok());
        assert!(output.check_write().is_ok());
    }

    #[test]
    fn test_capture_stream_isatty() {
        let buffer = new_capture_buffer();
        let stream = CaptureStream::new(buffer);
        assert!(!stream.isatty());
    }

    #[test]
    fn test_buffered_stdin_read() {
        let stream = BufferedStdin::new(b"hello world".to_vec());
        let mut input = stream.stream();

        // Read first 5 bytes
        let data = input.read(5).unwrap();
        assert_eq!(&data[..], b"hello");

        // Read remaining bytes
        let data = input.read(100).unwrap();
        assert_eq!(&data[..], b" world");

        // Read again should return empty (EOF)
        let data = input.read(100).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn test_empty_stdin() {
        let stream = EmptyStdin;
        let mut input = stream.stream();

        // Should return empty immediately
        let data = input.read(100).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn test_buffered_stdin_isatty() {
        let stream = BufferedStdin::new(vec![]);
        assert!(!stream.isatty());
    }
}
