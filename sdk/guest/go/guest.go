// Package isolate provides the Isolate guest SDK for Go.
//
// It handles JSON I/O protocol, environment access, and structured logging
// for WASM modules running inside Isolate sandboxes.
//
// # Quick Start
//
//	func main() {
//	    isolate.GuestMain(func(input MyInput) (MyOutput, error) {
//	        return MyOutput{Result: input.Value + 1}, nil
//	    })
//	}
package isolate

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
)

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

// GuestError wraps errors from guest SDK operations.
type GuestError struct {
	Message string
}

func (e *GuestError) Error() string {
	return fmt.Sprintf("guest error: %s", e.Message)
}

// NewGuestError creates a new GuestError.
func NewGuestError(msg string) *GuestError {
	return &GuestError{Message: msg}
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

// ReadInput reads and unmarshals JSON input from stdin.
func ReadInput[T any]() (T, error) {
	var result T
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return result, fmt.Errorf("read stdin: %w", err)
	}
	if len(data) == 0 {
		return result, nil
	}
	if err := json.Unmarshal(data, &result); err != nil {
		return result, &GuestError{Message: fmt.Sprintf("JSON parse error: %s", err)}
	}
	return result, nil
}

// ReadRaw reads raw bytes from stdin without JSON parsing.
func ReadRaw() ([]byte, error) {
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return nil, fmt.Errorf("read stdin: %w", err)
	}
	return data, nil
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

// WriteOutput marshals the value as JSON and writes it to stdout.
func WriteOutput[T any](output T) error {
	data, err := json.Marshal(output)
	if err != nil {
		return &GuestError{Message: fmt.Sprintf("JSON marshal error: %s", err)}
	}
	data = append(data, '\n')
	if _, err := os.Stdout.Write(data); err != nil {
		return fmt.Errorf("write stdout: %w", err)
	}
	return nil
}

// WriteRaw writes raw bytes to stdout without JSON encoding.
func WriteRaw(data []byte) error {
	if _, err := os.Stdout.Write(data); err != nil {
		return fmt.Errorf("write stdout: %w", err)
	}
	return nil
}

// ---------------------------------------------------------------------------
// Environment access
// ---------------------------------------------------------------------------

// GetEnv returns the value of an environment variable.
// Returns an empty string if the variable is not set or not permitted.
func GetEnv(name string) string {
	return os.Getenv(name)
}

// GetAllEnv returns all accessible environment variables as key-value pairs.
func GetAllEnv() map[string]string {
	result := make(map[string]string)
	for _, entry := range os.Environ() {
		for i := 0; i < len(entry); i++ {
			if entry[i] == '=' {
				result[entry[:i]] = entry[i+1:]
				break
			}
		}
	}
	return result
}

// GetArgs returns the command-line arguments passed to the sandbox.
func GetArgs() []string {
	return os.Args
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

// LogDebug writes a debug log message to stderr.
func LogDebug(msg string) {
	fmt.Fprintf(os.Stderr, "[DEBUG] %s\n", msg)
}

// LogInfo writes an informational log message to stderr.
func LogInfo(msg string) {
	fmt.Fprintf(os.Stderr, "[INFO] %s\n", msg)
}

// LogWarn writes a warning log message to stderr.
func LogWarn(msg string) {
	fmt.Fprintf(os.Stderr, "[WARN] %s\n", msg)
}

// LogError writes an error log message to stderr.
func LogError(msg string) {
	fmt.Fprintf(os.Stderr, "[ERROR] %s\n", msg)
}

// ---------------------------------------------------------------------------
// Main entry point helper
// ---------------------------------------------------------------------------

// GuestMain runs a function with Isolate JSON I/O protocol handling.
//
// It reads JSON input from stdin, calls the provided function, and writes
// the JSON result to stdout. On error, the error is logged to stderr and
// the process exits with code 1.
//
// Example:
//
//	func main() {
//	    isolate.GuestMain(func(input MyInput) (MyOutput, error) {
//	        return MyOutput{Greeting: "Hello, " + input.Name + "!"}, nil
//	    })
//	}
func GuestMain[I any, O any](f func(I) (O, error)) {
	input, err := ReadInput[I]()
	if err != nil {
		LogError(err.Error())
		os.Exit(1)
	}

	output, err := f(input)
	if err != nil {
		LogError(err.Error())
		os.Exit(1)
	}

	if err := WriteOutput(output); err != nil {
		LogError(err.Error())
		os.Exit(1)
	}
}
