//! Isolate Guest SDK for Rust
//!
//! Provides idiomatic Rust bindings for writing WASM modules that run inside
//! Isolate sandboxes. Handles JSON I/O protocol, logging, and error reporting.

use serde::{de::DeserializeOwned, Serialize};
use std::fmt;
use std::io::{self, Read, Write};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Error type for guest module operations.
#[derive(Debug)]
pub struct GuestError {
    message: String,
}

impl GuestError {
    /// Create a new guest error with the given message.
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for GuestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "guest error: {}", self.message)
    }
}

impl std::error::Error for GuestError {}

impl From<serde_json::Error> for GuestError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(format!("JSON error: {err}"))
    }
}

impl From<io::Error> for GuestError {
    fn from(err: io::Error) -> Self {
        Self::new(format!("I/O error: {err}"))
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Reads structured JSON input from stdin.
pub struct GuestInput;

impl GuestInput {
    /// Read and deserialize JSON input from stdin.
    ///
    /// # Errors
    ///
    /// Returns a `GuestError` if stdin cannot be read or the JSON is invalid.
    pub fn read<T: DeserializeOwned>() -> Result<T, GuestError> {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        let value = serde_json::from_slice(&buf)?;
        Ok(value)
    }

    /// Read raw bytes from stdin.
    pub fn read_raw() -> Result<Vec<u8>, GuestError> {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Writes structured JSON output to stdout.
pub struct GuestOutput;

impl GuestOutput {
    /// Serialize and write a value as JSON to stdout.
    ///
    /// # Errors
    ///
    /// Returns a `GuestError` if serialization or writing fails.
    pub fn write<T: Serialize>(value: &T) -> Result<(), GuestError> {
        let json = serde_json::to_string(value)?;
        let mut stdout = io::stdout().lock();
        stdout.write_all(json.as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        Ok(())
    }

    /// Write raw bytes to stdout.
    pub fn write_raw(data: &[u8]) -> Result<(), GuestError> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(data)?;
        stdout.flush()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Log an informational message to stderr.
pub fn log_info(msg: &str) {
    let _ = writeln!(io::stderr(), "[INFO] {msg}");
}

/// Log a warning message to stderr.
pub fn log_warn(msg: &str) {
    let _ = writeln!(io::stderr(), "[WARN] {msg}");
}

/// Log an error message to stderr.
pub fn log_error(msg: &str) {
    let _ = writeln!(io::stderr(), "[ERROR] {msg}");
}

// ---------------------------------------------------------------------------
// Main entry point helper
// ---------------------------------------------------------------------------

/// Run a guest function with JSON I/O protocol handling.
///
/// Reads JSON input from stdin, calls the provided function, and writes the
/// JSON result to stdout. On error, writes the error message to stderr and
/// exits with code 1.
///
/// # Example
///
/// ```ignore
/// #[no_mangle]
/// pub extern "C" fn _start() {
///     isolate_guest_rust::guest_main(|input: MyInput| {
///         Ok(MyOutput { result: input.value + 1 })
///     });
/// }
/// ```
pub fn guest_main<I, O, F>(f: F)
where
    I: DeserializeOwned,
    O: Serialize,
    F: FnOnce(I) -> Result<O, GuestError>,
{
    let result = (|| {
        let input = GuestInput::read::<I>()?;
        let output = f(input)?;
        GuestOutput::write(&output)?;
        Ok::<(), GuestError>(())
    })();

    if let Err(e) = result {
        log_error(&e.to_string());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guest_error_display() {
        let err = GuestError::new("something went wrong");
        assert_eq!(err.to_string(), "guest error: something went wrong");
    }

    #[test]
    fn test_guest_error_from_json() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let err = GuestError::from(json_err);
        assert!(err.to_string().contains("JSON error"));
    }
}
