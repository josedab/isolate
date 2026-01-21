package isolate

import "time"

// SandboxConfig holds the configuration for creating a sandbox.
type SandboxConfig struct {
	// MemoryLimit is the maximum heap memory in bytes.
	MemoryLimit uint64

	// FuelLimit is the maximum number of instructions (fuel units).
	FuelLimit uint64

	// WallTimeLimitSecs is the maximum wall-clock time in seconds.
	WallTimeLimitSecs uint32

	// CPUTimeLimitSecs is the maximum CPU time in seconds.
	CPUTimeLimitSecs uint32

	// Capabilities is the list of capabilities to grant the sandbox.
	Capabilities []Capability

	// Env is a map of environment variables to set in the sandbox.
	Env map[string]string

	// Args is the list of command-line arguments to pass to the module.
	Args []string
}

// Capability represents a sandbox capability (permission).
type Capability struct {
	// Type is the capability type (e.g., "stdout", "fs_read", "http").
	Type string

	// Value is an optional value for the capability (e.g., a path or host).
	Value string
}

// Stdout returns a capability that allows writing to stdout.
func Stdout() Capability {
	return Capability{Type: "stdout"}
}

// Stderr returns a capability that allows writing to stderr.
func Stderr() Capability {
	return Capability{Type: "stderr"}
}

// Stdin returns a capability that allows reading from stdin.
func Stdin() Capability {
	return Capability{Type: "stdin"}
}

// FsRead returns a capability that allows reading files under the given path.
func FsRead(path string) Capability {
	return Capability{Type: "fs_read", Value: path}
}

// FsWrite returns a capability that allows writing files under the given path.
func FsWrite(path string) Capability {
	return Capability{Type: "fs_write", Value: path}
}

// TempDir returns a capability that allows access to a temporary directory.
func TempDir() Capability {
	return Capability{Type: "temp_dir"}
}

// HTTP returns a capability that allows HTTP access to the given host pattern.
func HTTP(host string) Capability {
	return Capability{Type: "http", Value: host}
}

// DNS returns a capability that allows DNS resolution.
func DNS() Capability {
	return Capability{Type: "dns"}
}

// SystemClock returns a capability that allows access to the wall clock.
func SystemClock() Capability {
	return Capability{Type: "system_clock"}
}

// MonotonicClock returns a capability that allows access to the monotonic clock.
func MonotonicClock() Capability {
	return Capability{Type: "monotonic_clock"}
}

// Random returns a capability that allows access to cryptographic random numbers.
func Random() Capability {
	return Capability{Type: "random"}
}

// EnvVar returns a capability that allows access to a specific environment variable.
func EnvVar(name string) Capability {
	return Capability{Type: "env", Value: name}
}

// CreateSandboxResponse holds the result of creating a sandbox.
type CreateSandboxResponse struct {
	// SandboxID is the unique identifier for the created sandbox.
	SandboxID string

	// ModuleHash is the hash of the compiled WASM module.
	ModuleHash string

	// CreationTimeMs is the time taken to create the sandbox in milliseconds.
	CreationTimeMs float64
}

// RunSandboxRequest holds the parameters for running a sandbox.
type RunSandboxRequest struct {
	// Input is optional input data to pass via stdin.
	Input []byte

	// EntryPoint is the function to call (default: "_start").
	EntryPoint string
}

// RunSandboxResponse holds the result of running a sandbox.
type RunSandboxResponse struct {
	// ExitCode is the exit code returned by the WASM module.
	ExitCode int32

	// Stdout is the captured standard output.
	Stdout []byte

	// Stderr is the captured standard error.
	Stderr []byte

	// DurationMs is the execution duration in milliseconds.
	DurationMs float64

	// ResourceUsage holds the resource consumption details.
	ResourceUsage *ResourceUsage
}

// ResourceUsage holds resource consumption metrics for a sandbox run.
type ResourceUsage struct {
	// PeakMemory is the peak memory usage in bytes.
	PeakMemory uint64

	// FuelConsumed is the total fuel (instructions) consumed.
	FuelConsumed uint64

	// CPUTimeMs is the CPU time consumed in milliseconds.
	CPUTimeMs float64

	// WallTimeMs is the wall-clock time consumed in milliseconds.
	WallTimeMs float64

	// BytesRead is the total number of bytes read from I/O.
	BytesRead uint64

	// BytesWritten is the total number of bytes written to I/O.
	BytesWritten uint64
}

// SandboxInfo holds information about a sandbox.
type SandboxInfo struct {
	// ID is the unique identifier for the sandbox.
	ID string

	// State is the current state of the sandbox (e.g., "ready", "running").
	State string

	// ModuleHash is the hash of the compiled WASM module.
	ModuleHash string

	// CreatedAt is the creation timestamp.
	CreatedAt time.Time

	// AgeSecs is the age of the sandbox in seconds.
	AgeSecs float64

	// Metrics holds the sandbox execution metrics.
	Metrics *SandboxMetrics
}

// SandboxMetrics holds execution metrics for a sandbox.
type SandboxMetrics struct {
	// RunCount is the total number of runs.
	RunCount uint64

	// SuccessCount is the number of successful runs.
	SuccessCount uint64

	// FailureCount is the number of failed runs.
	FailureCount uint64

	// TotalRunDurationMs is the total time spent running in milliseconds.
	TotalRunDurationMs float64

	// LastRunDurationMs is the duration of the last run in milliseconds.
	LastRunDurationMs float64
}

// TerminateSandboxResponse holds the result of terminating a sandbox.
type TerminateSandboxResponse struct {
	// Terminated indicates whether the sandbox was successfully terminated.
	Terminated bool

	// Metrics holds the final execution metrics.
	Metrics *SandboxMetrics
}

// ListSandboxesRequest holds the parameters for listing sandboxes.
type ListSandboxesRequest struct {
	// StateFilter filters sandboxes by state. Empty string means no filter.
	StateFilter string

	// Limit is the maximum number of results to return. Zero means server default.
	Limit int32

	// Offset is the pagination offset.
	Offset int32
}

// ListSandboxesResponse holds the result of listing sandboxes.
type ListSandboxesResponse struct {
	// Sandboxes is the list of sandbox info objects.
	Sandboxes []SandboxInfo

	// Total is the total number of sandboxes matching the filter.
	Total int32
}
