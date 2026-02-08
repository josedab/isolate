// Package isolate provides a Go client for the Isolate gRPC sandbox service.
package isolate

import (
	"errors"
	"fmt"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Sentinel errors for common failure conditions.
var (
	// ErrConnectionFailed indicates the client could not connect to the server.
	ErrConnectionFailed = errors.New("isolate: connection failed")

	// ErrClientClosed indicates the client has already been closed.
	ErrClientClosed = errors.New("isolate: client is closed")

	// ErrSandboxNotFound indicates the requested sandbox does not exist.
	ErrSandboxNotFound = errors.New("isolate: sandbox not found")

	// ErrInvalidArgument indicates invalid input was provided.
	ErrInvalidArgument = errors.New("isolate: invalid argument")

	// ErrResourceExhausted indicates a resource limit was exceeded.
	ErrResourceExhausted = errors.New("isolate: resource exhausted")

	// ErrDeadlineExceeded indicates the operation timed out.
	ErrDeadlineExceeded = errors.New("isolate: deadline exceeded")

	// ErrPermissionDenied indicates a capability was not granted.
	ErrPermissionDenied = errors.New("isolate: permission denied")

	// ErrUnavailable indicates the server is unavailable.
	ErrUnavailable = errors.New("isolate: server unavailable")
)

// IsolateError wraps a gRPC or operational error with additional context.
type IsolateError struct {
	// Op is the operation that failed (e.g., "CreateSandbox", "RunSandbox").
	Op string

	// SandboxID is the sandbox ID involved, if applicable.
	SandboxID string

	// Code is the gRPC status code, if the error originated from gRPC.
	Code codes.Code

	// Err is the underlying error.
	Err error
}

// Error returns the string representation of the error.
func (e *IsolateError) Error() string {
	if e.SandboxID != "" {
		return fmt.Sprintf("isolate: %s (sandbox=%s): %v", e.Op, e.SandboxID, e.Err)
	}
	return fmt.Sprintf("isolate: %s: %v", e.Op, e.Err)
}

// Unwrap returns the underlying error for use with errors.Is and errors.As.
func (e *IsolateError) Unwrap() error {
	return e.Err
}

// wrapError converts a gRPC error into an IsolateError with the appropriate
// sentinel error as the underlying cause. If the error is nil, nil is returned.
func wrapError(op string, sandboxID string, err error) error {
	if err == nil {
		return nil
	}

	ie := &IsolateError{
		Op:        op,
		SandboxID: sandboxID,
		Err:       err,
	}

	st, ok := status.FromError(err)
	if !ok {
		return ie
	}

	ie.Code = st.Code()

	switch st.Code() {
	case codes.NotFound:
		ie.Err = fmt.Errorf("%w: %s", ErrSandboxNotFound, st.Message())
	case codes.InvalidArgument:
		ie.Err = fmt.Errorf("%w: %s", ErrInvalidArgument, st.Message())
	case codes.ResourceExhausted:
		ie.Err = fmt.Errorf("%w: %s", ErrResourceExhausted, st.Message())
	case codes.DeadlineExceeded:
		ie.Err = fmt.Errorf("%w: %s", ErrDeadlineExceeded, st.Message())
	case codes.PermissionDenied:
		ie.Err = fmt.Errorf("%w: %s", ErrPermissionDenied, st.Message())
	case codes.Unavailable:
		ie.Err = fmt.Errorf("%w: %s", ErrUnavailable, st.Message())
	}

	return ie
}

// IsNotFound reports whether the error indicates a sandbox was not found.
func IsNotFound(err error) bool {
	return errors.Is(err, ErrSandboxNotFound)
}

// IsInvalidArgument reports whether the error indicates invalid input.
func IsInvalidArgument(err error) bool {
	return errors.Is(err, ErrInvalidArgument)
}

// IsResourceExhausted reports whether the error indicates a resource limit was exceeded.
func IsResourceExhausted(err error) bool {
	return errors.Is(err, ErrResourceExhausted)
}

// IsDeadlineExceeded reports whether the error indicates a timeout.
func IsDeadlineExceeded(err error) bool {
	return errors.Is(err, ErrDeadlineExceeded)
}

// IsPermissionDenied reports whether the error indicates a missing capability.
func IsPermissionDenied(err error) bool {
	return errors.Is(err, ErrPermissionDenied)
}

// IsUnavailable reports whether the error indicates the server is unreachable.
func IsUnavailable(err error) bool {
	return errors.Is(err, ErrUnavailable)
}
