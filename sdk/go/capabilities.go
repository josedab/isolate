package isolate

// Capability represents a capability to grant to a sandbox.
type Capability struct {
	// Type is the capability type.
	Type string

	// Value is the capability value (path, host, etc.).
	Value string
}

// Stdout grants stdout access.
func Stdout() Capability {
	return Capability{Type: "stdout"}
}

// Stderr grants stderr access.
func Stderr() Capability {
	return Capability{Type: "stderr"}
}

// Stdin grants stdin access.
func Stdin() Capability {
	return Capability{Type: "stdin"}
}

// FsRead grants filesystem read access to the given path.
func FsRead(path string) Capability {
	return Capability{Type: "fs:read", Value: path}
}

// FsWrite grants filesystem write access to the given path.
func FsWrite(path string) Capability {
	return Capability{Type: "fs:write", Value: path}
}

// TempDir grants temp directory access.
func TempDir() Capability {
	return Capability{Type: "fs:temp"}
}

// HTTP grants HTTP client access to the given host pattern.
func HTTP(hostPattern string) Capability {
	return Capability{Type: "http", Value: hostPattern}
}

// DNS grants DNS resolution access.
func DNS() Capability {
	return Capability{Type: "dns"}
}

// SystemClock grants system clock access.
func SystemClock() Capability {
	return Capability{Type: "time:system"}
}

// MonotonicClock grants monotonic clock access.
func MonotonicClock() Capability {
	return Capability{Type: "time:monotonic"}
}

// Random grants secure random access.
func Random() Capability {
	return Capability{Type: "random"}
}

// Env grants access to the given environment variable.
func Env(varName string) Capability {
	return Capability{Type: "env", Value: varName}
}
