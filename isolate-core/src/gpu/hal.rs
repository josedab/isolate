//! Hardware Abstraction Layer (HAL) for GPU compute backends.
//!
//! Defines the `GpuBackend` trait that abstracts over different GPU hardware
//! implementations. The `SimulatedGpuBackend` is the default fallback when
//! no real hardware is available.
//!
//! To add a real backend (e.g., Vulkan):
//! 1. Implement `GpuBackend` for your backend struct
//! 2. Use `GpuContext::with_backend()` to inject it

use super::{
    BufferBinding, BufferUsage, ComputeResult, GpuCapabilities, GpuError, GpuLimits,
    ShaderValidation,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{Duration, Instant};

/// Result type for GPU HAL operations.
pub type HalResult<T> = Result<T, GpuError>;

/// Trait abstracting GPU hardware operations.
///
/// Implementations provide the actual compute dispatch, buffer management,
/// and shader compilation for a specific hardware platform.
pub trait GpuBackend: Send + Sync + Debug {
    /// Returns the backend name (e.g., "vulkan", "metal", "simulated").
    fn name(&self) -> &str;

    /// Returns the device capabilities.
    fn capabilities(&self) -> GpuCapabilities;

    /// Checks whether the backend is available on the current system.
    fn is_available(&self) -> bool;

    /// Allocates a buffer on the device and returns an opaque buffer handle.
    fn allocate_buffer(&mut self, size: u64, usage: BufferUsage) -> HalResult<BufferHandle>;

    /// Frees a previously allocated buffer.
    fn free_buffer(&mut self, handle: &BufferHandle) -> HalResult<()>;

    /// Writes data into a device buffer at the given offset.
    fn write_buffer(&mut self, handle: &BufferHandle, data: &[u8], offset: u64) -> HalResult<()>;

    /// Reads data from a device buffer.
    fn read_buffer(&self, handle: &BufferHandle, offset: u64, size: u64) -> HalResult<Vec<u8>>;

    /// Compiles a WGSL shader and returns an opaque shader handle.
    fn compile_shader(&mut self, source: &str, entry_point: &str) -> HalResult<ShaderHandle>;

    /// Validates a shader without compiling it.
    fn validate_shader(&self, source: &str) -> HalResult<ShaderValidation>;

    /// Dispatches a compute workload.
    fn dispatch(
        &mut self,
        shader: &ShaderHandle,
        workgroups: [u32; 3],
        bindings: &[HalBufferBinding],
    ) -> HalResult<ComputeResult>;

    /// Resets all device state, freeing all resources.
    fn reset(&mut self);

    /// Returns total device memory currently allocated.
    fn memory_used(&self) -> u64;
}

/// Opaque handle to a GPU buffer allocated by a backend.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BufferHandle {
    /// Backend-specific identifier.
    pub id: String,
    /// Buffer size in bytes.
    pub size: u64,
    /// Usage flags.
    pub usage: BufferUsage,
}

/// Opaque handle to a compiled shader.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShaderHandle {
    /// Backend-specific identifier.
    pub id: String,
    /// Entry point name.
    pub entry_point: String,
}

/// Buffer binding for dispatch, using HAL handles.
#[derive(Debug, Clone)]
pub struct HalBufferBinding {
    /// Binding index in the shader.
    pub binding: u32,
    /// Buffer handle.
    pub handle: BufferHandle,
    /// Offset into the buffer.
    pub offset: u64,
    /// Size of the bound region.
    pub size: u64,
}

impl From<(&BufferBinding, &BufferHandle)> for HalBufferBinding {
    fn from((binding, handle): (&BufferBinding, &BufferHandle)) -> Self {
        Self {
            binding: binding.binding,
            handle: handle.clone(),
            offset: binding.offset,
            size: binding.size,
        }
    }
}

// ---------------------------------------------------------------------------
// Simulated backend (default fallback)
// ---------------------------------------------------------------------------

/// Software-simulated GPU backend for development and testing.
#[derive(Debug)]
pub struct SimulatedGpuBackend {
    capabilities: GpuCapabilities,
    limits: GpuLimits,
    buffers: HashMap<String, Vec<u8>>,
    shaders: HashMap<String, String>,
    memory_used: u64,
    compute_time_used: Duration,
    next_id: u64,
}

impl SimulatedGpuBackend {
    /// Creates a new simulated backend with the given limits.
    pub fn new(limits: GpuLimits) -> Self {
        Self {
            capabilities: GpuCapabilities {
                device_name: "Simulated GPU (HAL)".to_string(),
                vendor: "Isolate".to_string(),
                ..Default::default()
            },
            limits,
            buffers: HashMap::new(),
            shaders: HashMap::new(),
            memory_used: 0,
            compute_time_used: Duration::ZERO,
            next_id: 0,
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{}-sim-{}", prefix, self.next_id)
    }
}

impl Default for SimulatedGpuBackend {
    fn default() -> Self {
        Self::new(GpuLimits::default())
    }
}

impl GpuBackend for SimulatedGpuBackend {
    fn name(&self) -> &str {
        "simulated"
    }

    fn capabilities(&self) -> GpuCapabilities {
        self.capabilities.clone()
    }

    fn is_available(&self) -> bool {
        true // Always available
    }

    fn allocate_buffer(&mut self, size: u64, usage: BufferUsage) -> HalResult<BufferHandle> {
        if self.memory_used + size > self.limits.max_memory {
            return Err(GpuError::OutOfMemory);
        }
        if size > self.capabilities.max_buffer_size {
            return Err(GpuError::BufferTooLarge(size));
        }

        let id = self.next_id("buf");
        self.buffers.insert(id.clone(), vec![0u8; size as usize]);
        self.memory_used += size;

        Ok(BufferHandle { id, size, usage })
    }

    fn free_buffer(&mut self, handle: &BufferHandle) -> HalResult<()> {
        self.buffers
            .remove(&handle.id)
            .ok_or_else(|| GpuError::BufferNotFound(handle.id.clone()))?;
        self.memory_used = self.memory_used.saturating_sub(handle.size);
        Ok(())
    }

    fn write_buffer(&mut self, handle: &BufferHandle, data: &[u8], offset: u64) -> HalResult<()> {
        let buf = self
            .buffers
            .get_mut(&handle.id)
            .ok_or_else(|| GpuError::BufferNotFound(handle.id.clone()))?;

        let start = offset as usize;
        let end = start + data.len();
        if end > buf.len() {
            return Err(GpuError::BufferOverflow);
        }

        buf[start..end].copy_from_slice(data);
        Ok(())
    }

    fn read_buffer(&self, handle: &BufferHandle, offset: u64, size: u64) -> HalResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.id)
            .ok_or_else(|| GpuError::BufferNotFound(handle.id.clone()))?;

        let start = offset as usize;
        let end = start + size as usize;
        if end > buf.len() {
            return Err(GpuError::BufferOverflow);
        }

        Ok(buf[start..end].to_vec())
    }

    fn compile_shader(&mut self, source: &str, entry_point: &str) -> HalResult<ShaderHandle> {
        let validation = self.validate_shader(source)?;
        if !validation.valid {
            return Err(GpuError::ShaderCompilationFailed(
                validation.errors.first().map(|e| e.message.clone()).unwrap_or_default(),
            ));
        }

        if validation.instruction_count > self.limits.max_shader_instructions {
            return Err(GpuError::ShaderTooComplex);
        }

        let id = self.next_id("shader");
        self.shaders.insert(id.clone(), source.to_string());

        Ok(ShaderHandle { id, entry_point: entry_point.to_string() })
    }

    fn validate_shader(&self, source: &str) -> HalResult<ShaderValidation> {
        use super::ShaderError;
        let mut errors = Vec::new();
        let warnings = Vec::new();

        if source.is_empty() {
            errors.push(ShaderError {
                message: "Empty shader source".to_string(),
                line: None,
                column: None,
            });
        }

        let instruction_count = source.lines().count() as u64 * 10;

        Ok(ShaderValidation {
            valid: errors.is_empty(),
            errors,
            warnings,
            instruction_count,
            memory_estimate: instruction_count * 8,
        })
    }

    fn dispatch(
        &mut self,
        shader: &ShaderHandle,
        workgroups: [u32; 3],
        bindings: &[HalBufferBinding],
    ) -> HalResult<ComputeResult> {
        if !self.shaders.contains_key(&shader.id) {
            return Err(GpuError::ShaderNotFound(shader.id.clone()));
        }

        let total = workgroups[0] as u64 * workgroups[1] as u64 * workgroups[2] as u64;
        if total > self.limits.max_workgroups as u64 {
            return Err(GpuError::TooManyWorkgroups);
        }

        for b in bindings {
            if !self.buffers.contains_key(&b.handle.id) {
                return Err(GpuError::BufferNotFound(b.handle.id.clone()));
            }
        }

        if self.compute_time_used >= self.limits.max_compute_time {
            return Err(GpuError::ComputeTimeExceeded);
        }

        let start = Instant::now();
        let sim_time = Duration::from_micros(total.max(1));
        self.compute_time_used += sim_time;

        Ok(ComputeResult {
            success: true,
            execution_time: start.elapsed(),
            memory_used: self.memory_used,
            error: None,
        })
    }

    fn reset(&mut self) {
        self.buffers.clear();
        self.shaders.clear();
        self.memory_used = 0;
        self.compute_time_used = Duration::ZERO;
    }

    fn memory_used(&self) -> u64 {
        self.memory_used
    }
}

// ---------------------------------------------------------------------------
// Backend registry
// ---------------------------------------------------------------------------

/// Detects the best available GPU backend on the current system.
///
/// Priority: Vulkan > Metal > Simulated. Currently only returns `Simulated`
/// since real backends require feature-gated hardware crates.
pub fn detect_gpu_backend(limits: GpuLimits) -> Box<dyn GpuBackend> {
    // Future: probe for Vulkan, Metal, etc.
    Box::new(SimulatedGpuBackend::new(limits))
}

/// Lists all GPU backends known to this build.
pub fn available_backends() -> Vec<&'static str> {
    let backends = vec!["simulated"];

    // Feature-gated real backends would be appended here:
    // #[cfg(feature = "gpu-vulkan")]  backends.push("vulkan");
    // #[cfg(feature = "gpu-metal")]   backends.push("metal");
    backends
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> SimulatedGpuBackend {
        SimulatedGpuBackend::new(GpuLimits::default())
    }

    #[test]
    fn test_backend_name() {
        assert_eq!(backend().name(), "simulated");
    }

    #[test]
    fn test_backend_always_available() {
        assert!(backend().is_available());
    }

    #[test]
    fn test_capabilities_returned() {
        let caps = backend().capabilities();
        assert!(caps.device_name.contains("Simulated"));
        assert!(caps.max_buffer_size > 0);
    }

    #[test]
    fn test_allocate_and_free_buffer() {
        let mut b = backend();
        let handle = b.allocate_buffer(1024, BufferUsage::default()).unwrap();
        assert_eq!(b.memory_used(), 1024);

        b.free_buffer(&handle).unwrap();
        assert_eq!(b.memory_used(), 0);
    }

    #[test]
    fn test_allocate_over_limit() {
        let mut b = SimulatedGpuBackend::new(GpuLimits { max_memory: 512, ..Default::default() });
        let result = b.allocate_buffer(1024, BufferUsage::default());
        assert!(matches!(result, Err(GpuError::OutOfMemory)));
    }

    #[test]
    fn test_write_and_read_buffer() {
        let mut b = backend();
        let usage = BufferUsage { map_read: true, map_write: true, ..Default::default() };
        let handle = b.allocate_buffer(256, usage).unwrap();

        let data = vec![42u8; 64];
        b.write_buffer(&handle, &data, 0).unwrap();

        let readback = b.read_buffer(&handle, 0, 64).unwrap();
        assert_eq!(readback, data);
    }

    #[test]
    fn test_write_buffer_overflow() {
        let mut b = backend();
        let handle = b.allocate_buffer(16, BufferUsage::default()).unwrap();
        let result = b.write_buffer(&handle, &[0u8; 32], 0);
        assert!(matches!(result, Err(GpuError::BufferOverflow)));
    }

    #[test]
    fn test_compile_valid_shader() {
        let mut b = backend();
        let handle = b.compile_shader("@compute fn main() {}", "main").unwrap();
        assert!(!handle.id.is_empty());
        assert_eq!(handle.entry_point, "main");
    }

    #[test]
    fn test_compile_empty_shader_fails() {
        let mut b = backend();
        let result = b.compile_shader("", "main");
        assert!(matches!(result, Err(GpuError::ShaderCompilationFailed(_))));
    }

    #[test]
    fn test_dispatch_compute() {
        let mut b = backend();
        let buf_handle = b.allocate_buffer(1024, BufferUsage::default()).unwrap();
        let shader_handle = b.compile_shader("@compute fn main() {}", "main").unwrap();

        let binding = HalBufferBinding { binding: 0, handle: buf_handle, offset: 0, size: 1024 };

        let result = b.dispatch(&shader_handle, [8, 1, 1], &[binding]).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_dispatch_missing_shader() {
        let mut b = backend();
        let fake = ShaderHandle { id: "fake".to_string(), entry_point: "main".to_string() };
        let result = b.dispatch(&fake, [1, 1, 1], &[]);
        assert!(matches!(result, Err(GpuError::ShaderNotFound(_))));
    }

    #[test]
    fn test_dispatch_workgroup_limit() {
        let mut b =
            SimulatedGpuBackend::new(GpuLimits { max_workgroups: 10, ..Default::default() });
        let sh = b.compile_shader("@compute fn main() {}", "main").unwrap();
        let result = b.dispatch(&sh, [100, 100, 100], &[]);
        assert!(matches!(result, Err(GpuError::TooManyWorkgroups)));
    }

    #[test]
    fn test_reset_clears_all() {
        let mut b = backend();
        b.allocate_buffer(512, BufferUsage::default()).unwrap();
        b.compile_shader("@compute fn main() {}", "main").unwrap();
        assert!(b.memory_used() > 0);

        b.reset();
        assert_eq!(b.memory_used(), 0);
    }

    #[test]
    fn test_detect_returns_simulated() {
        let b = detect_gpu_backend(GpuLimits::default());
        assert_eq!(b.name(), "simulated");
        assert!(b.is_available());
    }

    #[test]
    fn test_available_backends_includes_simulated() {
        let backends = available_backends();
        assert!(backends.contains(&"simulated"));
    }

    #[test]
    fn test_validate_shader_standalone() {
        let b = backend();
        let v = b.validate_shader("@compute fn test() { let x = 1; }").unwrap();
        assert!(v.valid);
        assert!(v.instruction_count > 0);
    }

    #[test]
    fn test_free_unknown_buffer_fails() {
        let mut b = backend();
        let fake =
            BufferHandle { id: "nonexistent".to_string(), size: 0, usage: BufferUsage::default() };
        assert!(matches!(b.free_buffer(&fake), Err(GpuError::BufferNotFound(_))));
    }
}
