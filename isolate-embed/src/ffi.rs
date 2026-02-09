//! C FFI bindings for the embedded sandbox.
//!
//! Provides `extern "C"` functions for creating and running WASM sandboxes
//! from C/C++ code.
//!
//! # Usage from C
//!
//! ```c
//! #include "isolate_embed.h"
//!
//! IsolateConfig *config = isolate_config_new(wasm_bytes, wasm_len);
//! isolate_config_set_memory_limit(config, 64 * 1024 * 1024);
//! isolate_config_set_fuel(config, 1000000);
//!
//! IsolateSandbox *sandbox = isolate_sandbox_create(config);
//! IsolateOutput *output = isolate_sandbox_run(sandbox, NULL, 0);
//!
//! int exit_code = isolate_output_exit_code(output);
//! const char *stdout = isolate_output_stdout(output);
//!
//! isolate_output_free(output);
//! isolate_sandbox_free(sandbox);
//! isolate_config_free(config);
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

use crate::{Output, Sandbox, SandboxConfig};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

/// Opaque sandbox configuration handle.
pub struct IsolateConfig {
    inner: SandboxConfig,
}

/// Opaque sandbox handle.
pub struct IsolateSandbox {
    inner: Sandbox,
}

/// Opaque output handle.
pub struct IsolateOutput {
    inner: Output,
    stdout_cstr: Option<CString>,
    stderr_cstr: Option<CString>,
}

/// Create a new sandbox configuration from WASM bytes.
///
/// # Safety
/// `wasm_bytes` must point to a valid buffer of at least `wasm_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn isolate_config_new(
    wasm_bytes: *const u8,
    wasm_len: usize,
) -> *mut IsolateConfig {
    if wasm_bytes.is_null() || wasm_len == 0 {
        return ptr::null_mut();
    }

    let bytes = unsafe { std::slice::from_raw_parts(wasm_bytes, wasm_len) };
    let config = SandboxConfig::new(bytes);
    Box::into_raw(Box::new(IsolateConfig { inner: config }))
}

/// Set memory limit on a config.
///
/// # Safety
/// `config` must be a valid pointer from `isolate_config_new`.
#[no_mangle]
pub unsafe extern "C" fn isolate_config_set_memory_limit(
    config: *mut IsolateConfig,
    bytes: usize,
) {
    if let Some(c) = unsafe { config.as_mut() } {
        c.inner = c.inner.clone().memory_limit(bytes);
    }
}

/// Set fuel limit on a config.
///
/// # Safety
/// `config` must be a valid pointer from `isolate_config_new`.
#[no_mangle]
pub unsafe extern "C" fn isolate_config_set_fuel(config: *mut IsolateConfig, fuel: u64) {
    if let Some(c) = unsafe { config.as_mut() } {
        c.inner = c.inner.clone().fuel(fuel);
    }
}

/// Set an environment variable on a config.
///
/// # Safety
/// `config`, `key`, and `value` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn isolate_config_set_env(
    config: *mut IsolateConfig,
    key: *const c_char,
    value: *const c_char,
) {
    if config.is_null() || key.is_null() || value.is_null() {
        return;
    }
    if let (Some(c), Ok(k), Ok(v)) = (
        unsafe { config.as_mut() },
        unsafe { CStr::from_ptr(key) }.to_str(),
        unsafe { CStr::from_ptr(value) }.to_str(),
    ) {
        c.inner = c.inner.clone().env(k, v);
    }
}

/// Free a config handle.
///
/// # Safety
/// `config` must be a valid pointer from `isolate_config_new` or null.
#[no_mangle]
pub unsafe extern "C" fn isolate_config_free(config: *mut IsolateConfig) {
    if !config.is_null() {
        drop(unsafe { Box::from_raw(config) });
    }
}

/// Create a sandbox from a config. Returns null on error.
///
/// # Safety
/// `config` must be a valid pointer from `isolate_config_new`.
#[no_mangle]
pub unsafe extern "C" fn isolate_sandbox_create(
    config: *mut IsolateConfig,
) -> *mut IsolateSandbox {
    let Some(c) = (unsafe { config.as_ref() }) else {
        return ptr::null_mut();
    };

    match Sandbox::create(c.inner.clone()) {
        Ok(sandbox) => Box::into_raw(Box::new(IsolateSandbox { inner: sandbox })),
        Err(_) => ptr::null_mut(),
    }
}

/// Run the sandbox with optional input. Returns null on error.
///
/// # Safety
/// `sandbox` must be a valid pointer. `input` may be null if `input_len` is 0.
#[no_mangle]
pub unsafe extern "C" fn isolate_sandbox_run(
    sandbox: *mut IsolateSandbox,
    input: *const u8,
    input_len: usize,
) -> *mut IsolateOutput {
    let Some(sb) = (unsafe { sandbox.as_mut() }) else {
        return ptr::null_mut();
    };

    let input_slice = if input.is_null() || input_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len) }
    };

    match sb.inner.run(input_slice) {
        Ok(output) => Box::into_raw(Box::new(IsolateOutput {
            inner: output,
            stdout_cstr: None,
            stderr_cstr: None,
        })),
        Err(_) => ptr::null_mut(),
    }
}

/// Free a sandbox handle.
///
/// # Safety
/// `sandbox` must be a valid pointer from `isolate_sandbox_create` or null.
#[no_mangle]
pub unsafe extern "C" fn isolate_sandbox_free(sandbox: *mut IsolateSandbox) {
    if !sandbox.is_null() {
        drop(unsafe { Box::from_raw(sandbox) });
    }
}

/// Get exit code from output.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_exit_code(output: *const IsolateOutput) -> c_int {
    (unsafe { output.as_ref() }).map_or(-1, |o| o.inner.exit_code)
}

/// Get stdout as a null-terminated C string. Valid until output is freed.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_stdout(output: *mut IsolateOutput) -> *const c_char {
    let Some(o) = (unsafe { output.as_mut() }) else {
        return ptr::null();
    };
    if o.stdout_cstr.is_none() {
        let s = String::from_utf8_lossy(&o.inner.stdout).into_owned();
        o.stdout_cstr = CString::new(s).ok();
    }
    o.stdout_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr())
}

/// Get stderr as a null-terminated C string. Valid until output is freed.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_stderr(output: *mut IsolateOutput) -> *const c_char {
    let Some(o) = (unsafe { output.as_mut() }) else {
        return ptr::null();
    };
    if o.stderr_cstr.is_none() {
        let s = String::from_utf8_lossy(&o.inner.stderr).into_owned();
        o.stderr_cstr = CString::new(s).ok();
    }
    o.stderr_cstr.as_ref().map_or(ptr::null(), |c| c.as_ptr())
}

/// Get stdout length in bytes.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_stdout_len(output: *const IsolateOutput) -> usize {
    (unsafe { output.as_ref() }).map_or(0, |o| o.inner.stdout.len())
}

/// Get execution duration in milliseconds.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_duration_ms(output: *const IsolateOutput) -> u64 {
    (unsafe { output.as_ref() }).map_or(0, |o| o.inner.duration.as_millis() as u64)
}

/// Get fuel consumed.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run`.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_fuel_consumed(output: *const IsolateOutput) -> u64 {
    (unsafe { output.as_ref() }).map_or(0, |o| o.inner.fuel_consumed)
}

/// Free an output handle.
///
/// # Safety
/// `output` must be a valid pointer from `isolate_sandbox_run` or null.
#[no_mangle]
pub unsafe extern "C" fn isolate_output_free(output: *mut IsolateOutput) {
    if !output.is_null() {
        drop(unsafe { Box::from_raw(output) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_config_lifecycle() {
        unsafe {
            let config = isolate_config_new(MINIMAL_WASM.as_ptr(), MINIMAL_WASM.len());
            assert!(!config.is_null());
            isolate_config_set_memory_limit(config, 32 * 1024 * 1024);
            isolate_config_set_fuel(config, 500_000);
            isolate_config_free(config);
        }
    }

    #[test]
    fn test_null_safety() {
        unsafe {
            assert!(isolate_config_new(ptr::null(), 0).is_null());
            assert!(isolate_sandbox_create(ptr::null_mut()).is_null());
            assert!(isolate_sandbox_run(ptr::null_mut(), ptr::null(), 0).is_null());
            assert_eq!(isolate_output_exit_code(ptr::null()), -1);
            assert!(isolate_output_stdout(ptr::null_mut()).is_null());

            // Free null pointers should be safe no-ops
            isolate_config_free(ptr::null_mut());
            isolate_sandbox_free(ptr::null_mut());
            isolate_output_free(ptr::null_mut());
        }
    }
}
