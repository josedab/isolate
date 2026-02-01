// Package isolate provides the Isolate guest SDK for Go.
//
// It handles JSON I/O protocol, environment access, and structured logging
// for WASM modules running inside Isolate sandboxes.
package isolate

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
)

// ReadInput reads and unmarshals JSON input from stdin.
func ReadInput[T any]() (T, error) {
	var result T
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return result, fmt.Errorf("read stdin: %w", err)
	}
	if err := json.Unmarshal(data, &result); err != nil {
		return result, fmt.Errorf("unmarshal input: %w", err)
	}
	return result, nil
}

// WriteOutput marshals the value as JSON and writes it to stdout.
func WriteOutput[T any](output T) error {
	data, err := json.Marshal(output)
	if err != nil {
		return fmt.Errorf("marshal output: %w", err)
	}
	data = append(data, '\n')
	if _, err := os.Stdout.Write(data); err != nil {
		return fmt.Errorf("write stdout: %w", err)
	}
	return nil
}

// GetEnv returns the value of an environment variable.
// Returns an empty string if the variable is not set or not permitted.
func GetEnv(name string) string {
	return os.Getenv(name)
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
