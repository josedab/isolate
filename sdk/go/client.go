// Package isolate provides a Go client SDK for the Isolate secure sandbox runtime.
//
// Example usage:
//
//	client, err := isolate.NewClient("localhost:50051")
//	if err != nil {
//	    log.Fatal(err)
//	}
//	defer client.Close()
//
//	wasmBytes, _ := os.ReadFile("module.wasm")
//	result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
//	    MemoryLimit: 64 * 1024 * 1024,
//	    Capabilities: []isolate.Capability{
//	        isolate.Stdout(),
//	        isolate.Stderr(),
//	    },
//	})
package isolate

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"time"

	pb "github.com/josedab/isolate/sdk/go/isolate/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
)

// Client is the main Isolate client for interacting with the sandbox server.
type Client struct {
	conn   *grpc.ClientConn
	client pb.IsolateServiceClient
}

// ClientOptions configures the Isolate client connection.
type ClientOptions struct {
	// TLS enables TLS for the connection.
	TLS bool

	// RootCAs is the root certificate pool for TLS verification.
	RootCAs *x509.CertPool

	// ClientCert is the client certificate for mTLS.
	ClientCert *tls.Certificate

	// DialTimeout is the connection timeout.
	DialTimeout time.Duration

	// AdditionalDialOptions are extra gRPC dial options.
	AdditionalDialOptions []grpc.DialOption
}

// NewClient creates a new Isolate client connected to the given address.
func NewClient(address string, opts ...ClientOptions) (*Client, error) {
	var options ClientOptions
	if len(opts) > 0 {
		options = opts[0]
	}

	dialOpts := []grpc.DialOption{}

	// Configure TLS
	if options.TLS {
		tlsConfig := &tls.Config{
			RootCAs: options.RootCAs,
		}
		if options.ClientCert != nil {
			tlsConfig.Certificates = []tls.Certificate{*options.ClientCert}
		}
		dialOpts = append(dialOpts, grpc.WithTransportCredentials(credentials.NewTLS(tlsConfig)))
	} else {
		dialOpts = append(dialOpts, grpc.WithTransportCredentials(insecure.NewCredentials()))
	}

	// Add additional dial options
	dialOpts = append(dialOpts, options.AdditionalDialOptions...)

	// Set dial timeout context
	ctx := context.Background()
	if options.DialTimeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, options.DialTimeout)
		defer cancel()
	}

	conn, err := grpc.DialContext(ctx, address, dialOpts...)
	if err != nil {
		return nil, fmt.Errorf("failed to connect to %s: %w", address, err)
	}

	return &Client{
		conn:   conn,
		client: pb.NewIsolateServiceClient(conn),
	}, nil
}

// Close closes the client connection.
func (c *Client) Close() error {
	return c.conn.Close()
}

// CreateSandboxOptions configures sandbox creation.
type CreateSandboxOptions struct {
	// MemoryLimit is the maximum memory in bytes.
	MemoryLimit uint64

	// FuelLimit is the CPU fuel limit (instruction count).
	FuelLimit uint64

	// WallTimeLimitSecs is the wall-clock time limit.
	WallTimeLimitSecs uint32

	// CPUTimeLimitSecs is the CPU time limit.
	CPUTimeLimitSecs uint32

	// Capabilities are the capabilities to grant.
	Capabilities []Capability

	// Env is environment variables to pass.
	Env map[string]string

	// Args are command-line arguments to pass.
	Args []string
}

// CreateSandboxResult contains the result of creating a sandbox.
type CreateSandboxResult struct {
	// SandboxID is the unique sandbox identifier.
	SandboxID string

	// ModuleHash is the hash of the WASM module.
	ModuleHash string

	// CreationTimeMs is the time taken to create the sandbox.
	CreationTimeMs float64
}

// CreateSandbox creates a new sandbox with the given WASM module.
func (c *Client) CreateSandbox(ctx context.Context, module []byte, opts *CreateSandboxOptions) (*CreateSandboxResult, error) {
	if opts == nil {
		opts = &CreateSandboxOptions{}
	}

	capabilities := make([]*pb.Capability, len(opts.Capabilities))
	for i, cap := range opts.Capabilities {
		capabilities[i] = &pb.Capability{
			Type:  cap.Type,
			Value: cap.Value,
		}
	}

	resp, err := c.client.CreateSandbox(ctx, &pb.CreateSandboxRequest{
		Module: module,
		Config: &pb.SandboxConfig{
			MemoryLimit:       opts.MemoryLimit,
			FuelLimit:         opts.FuelLimit,
			WallTimeLimitSecs: opts.WallTimeLimitSecs,
			CpuTimeLimitSecs:  opts.CPUTimeLimitSecs,
			Capabilities:      capabilities,
			Env:               opts.Env,
			Args:              opts.Args,
		},
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create sandbox: %w", err)
	}

	return &CreateSandboxResult{
		SandboxID:      resp.SandboxId,
		ModuleHash:     resp.ModuleHash,
		CreationTimeMs: resp.CreationTimeMs,
	}, nil
}

// RunSandboxOptions configures sandbox execution.
type RunSandboxOptions struct {
	// Input is the data to provide to stdin.
	Input []byte

	// EntryPoint is the entry point function name.
	// Defaults to "_start".
	EntryPoint string
}

// RunSandboxResult contains the result of running a sandbox.
type RunSandboxResult struct {
	// ExitCode is the exit code from the WASM module.
	ExitCode int32

	// Stdout is the captured stdout.
	Stdout []byte

	// Stderr is the captured stderr.
	Stderr []byte

	// DurationMs is the execution duration.
	DurationMs float64

	// ResourceUsage contains resource usage information.
	ResourceUsage *ResourceUsage
}

// ResourceUsage contains resource usage information.
type ResourceUsage struct {
	// PeakMemory is the peak memory usage in bytes.
	PeakMemory uint64

	// FuelConsumed is the CPU fuel consumed.
	FuelConsumed uint64

	// CPUTimeMs is the CPU time in milliseconds.
	CPUTimeMs float64

	// WallTimeMs is the wall-clock time in milliseconds.
	WallTimeMs float64

	// BytesRead is the total bytes read.
	BytesRead uint64

	// BytesWritten is the total bytes written.
	BytesWritten uint64
}

// RunSandbox runs an existing sandbox.
func (c *Client) RunSandbox(ctx context.Context, sandboxID string, opts *RunSandboxOptions) (*RunSandboxResult, error) {
	if opts == nil {
		opts = &RunSandboxOptions{}
	}

	entryPoint := opts.EntryPoint
	if entryPoint == "" {
		entryPoint = "_start"
	}

	resp, err := c.client.RunSandbox(ctx, &pb.RunSandboxRequest{
		SandboxId:  sandboxID,
		Input:      opts.Input,
		EntryPoint: entryPoint,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to run sandbox: %w", err)
	}

	result := &RunSandboxResult{
		ExitCode:   resp.ExitCode,
		Stdout:     resp.Stdout,
		Stderr:     resp.Stderr,
		DurationMs: resp.DurationMs,
	}

	if resp.ResourceUsage != nil {
		result.ResourceUsage = &ResourceUsage{
			PeakMemory:   resp.ResourceUsage.PeakMemory,
			FuelConsumed: resp.ResourceUsage.FuelConsumed,
			CPUTimeMs:    resp.ResourceUsage.CpuTimeMs,
			WallTimeMs:   resp.ResourceUsage.WallTimeMs,
			BytesRead:    resp.ResourceUsage.BytesRead,
			BytesWritten: resp.ResourceUsage.BytesWritten,
		}
	}

	return result, nil
}

// SandboxInfo contains information about a sandbox.
type SandboxInfo struct {
	// ID is the sandbox identifier.
	ID string

	// State is the current sandbox state.
	State string

	// ModuleHash is the hash of the WASM module.
	ModuleHash string

	// CreatedAt is the creation timestamp.
	CreatedAt time.Time

	// AgeSecs is the age in seconds.
	AgeSecs float64

	// Metrics contains execution metrics.
	Metrics *SandboxMetrics
}

// SandboxMetrics contains sandbox execution metrics.
type SandboxMetrics struct {
	// RunCount is the total number of runs.
	RunCount uint64

	// SuccessCount is the number of successful runs.
	SuccessCount uint64

	// FailureCount is the number of failed runs.
	FailureCount uint64

	// TotalRunDurationMs is the total run duration.
	TotalRunDurationMs float64

	// LastRunDurationMs is the last run duration.
	LastRunDurationMs float64
}

// GetSandbox gets sandbox status and metrics.
func (c *Client) GetSandbox(ctx context.Context, sandboxID string) (*SandboxInfo, error) {
	resp, err := c.client.GetSandbox(ctx, &pb.GetSandboxRequest{
		SandboxId: sandboxID,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to get sandbox: %w", err)
	}

	sandbox := resp.Sandbox
	info := &SandboxInfo{
		ID:         sandbox.Id,
		State:      sandbox.State,
		ModuleHash: sandbox.ModuleHash,
		CreatedAt:  time.Unix(sandbox.CreatedAt, 0),
		AgeSecs:    sandbox.AgeSecs,
	}

	if sandbox.Metrics != nil {
		info.Metrics = &SandboxMetrics{
			RunCount:           sandbox.Metrics.RunCount,
			SuccessCount:       sandbox.Metrics.SuccessCount,
			FailureCount:       sandbox.Metrics.FailureCount,
			TotalRunDurationMs: sandbox.Metrics.TotalRunDurationMs,
			LastRunDurationMs:  sandbox.Metrics.LastRunDurationMs,
		}
	}

	return info, nil
}

// TerminateSandboxResult contains the result of terminating a sandbox.
type TerminateSandboxResult struct {
	// Terminated indicates if the sandbox was terminated.
	Terminated bool

	// Metrics contains final execution metrics.
	Metrics *SandboxMetrics
}

// TerminateSandbox terminates a sandbox.
func (c *Client) TerminateSandbox(ctx context.Context, sandboxID string) (*TerminateSandboxResult, error) {
	resp, err := c.client.TerminateSandbox(ctx, &pb.TerminateSandboxRequest{
		SandboxId: sandboxID,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to terminate sandbox: %w", err)
	}

	result := &TerminateSandboxResult{
		Terminated: resp.Terminated,
	}

	if resp.Metrics != nil {
		result.Metrics = &SandboxMetrics{
			RunCount:           resp.Metrics.RunCount,
			SuccessCount:       resp.Metrics.SuccessCount,
			FailureCount:       resp.Metrics.FailureCount,
			TotalRunDurationMs: resp.Metrics.TotalRunDurationMs,
			LastRunDurationMs:  resp.Metrics.LastRunDurationMs,
		}
	}

	return result, nil
}

// ListSandboxesOptions configures sandbox listing.
type ListSandboxesOptions struct {
	// StateFilter filters by sandbox state.
	StateFilter string

	// Limit is the maximum number of results.
	Limit int32

	// Offset is the pagination offset.
	Offset int32
}

// ListSandboxesResult contains the result of listing sandboxes.
type ListSandboxesResult struct {
	// Sandboxes is the list of sandboxes.
	Sandboxes []*SandboxInfo

	// Total is the total count.
	Total int32
}

// ListSandboxes lists all sandboxes.
func (c *Client) ListSandboxes(ctx context.Context, opts *ListSandboxesOptions) (*ListSandboxesResult, error) {
	if opts == nil {
		opts = &ListSandboxesOptions{}
	}

	resp, err := c.client.ListSandboxes(ctx, &pb.ListSandboxesRequest{
		StateFilter: opts.StateFilter,
		Limit:       opts.Limit,
		Offset:      opts.Offset,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to list sandboxes: %w", err)
	}

	sandboxes := make([]*SandboxInfo, len(resp.Sandboxes))
	for i, s := range resp.Sandboxes {
		info := &SandboxInfo{
			ID:         s.Id,
			State:      s.State,
			ModuleHash: s.ModuleHash,
			CreatedAt:  time.Unix(s.CreatedAt, 0),
			AgeSecs:    s.AgeSecs,
		}
		if s.Metrics != nil {
			info.Metrics = &SandboxMetrics{
				RunCount:           s.Metrics.RunCount,
				SuccessCount:       s.Metrics.SuccessCount,
				FailureCount:       s.Metrics.FailureCount,
				TotalRunDurationMs: s.Metrics.TotalRunDurationMs,
				LastRunDurationMs:  s.Metrics.LastRunDurationMs,
			}
		}
		sandboxes[i] = info
	}

	return &ListSandboxesResult{
		Sandboxes: sandboxes,
		Total:     resp.Total,
	}, nil
}

// GetMetrics gets server metrics.
func (c *Client) GetMetrics(ctx context.Context, format string) (string, error) {
	if format == "" {
		format = "prometheus"
	}

	resp, err := c.client.GetMetrics(ctx, &pb.GetMetricsRequest{
		Format: format,
	})
	if err != nil {
		return "", fmt.Errorf("failed to get metrics: %w", err)
	}

	return resp.Data, nil
}

// ExecuteOptions combines create and run options for convenience.
type ExecuteOptions struct {
	// CreateSandboxOptions
	MemoryLimit       uint64
	FuelLimit         uint64
	WallTimeLimitSecs uint32
	CPUTimeLimitSecs  uint32
	Capabilities      []Capability
	Env               map[string]string
	Args              []string

	// RunSandboxOptions
	Input      []byte
	EntryPoint string
}

// Execute creates, runs, and terminates a sandbox in one call.
func (c *Client) Execute(ctx context.Context, module []byte, opts *ExecuteOptions) (*RunSandboxResult, error) {
	if opts == nil {
		opts = &ExecuteOptions{}
	}

	// Create sandbox
	createResult, err := c.CreateSandbox(ctx, module, &CreateSandboxOptions{
		MemoryLimit:       opts.MemoryLimit,
		FuelLimit:         opts.FuelLimit,
		WallTimeLimitSecs: opts.WallTimeLimitSecs,
		CPUTimeLimitSecs:  opts.CPUTimeLimitSecs,
		Capabilities:      opts.Capabilities,
		Env:               opts.Env,
		Args:              opts.Args,
	})
	if err != nil {
		return nil, err
	}

	// Run sandbox
	result, err := c.RunSandbox(ctx, createResult.SandboxID, &RunSandboxOptions{
		Input:      opts.Input,
		EntryPoint: opts.EntryPoint,
	})

	// Always terminate, even on error
	_, _ = c.TerminateSandbox(ctx, createResult.SandboxID)

	if err != nil {
		return nil, err
	}

	return result, nil
}
