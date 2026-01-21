// Package isolate provides a Go client for the Isolate gRPC sandbox service.
//
// The client supports creating, running, inspecting, and terminating WASM
// sandboxes with capability-based security and resource controls.
//
// Basic usage:
//
//	client, err := isolate.NewClient("localhost:50051")
//	if err != nil {
//	    log.Fatal(err)
//	}
//	defer client.Close()
//
//	ctx := context.Background()
//	resp, err := client.CreateSandbox(ctx, wasmBytes, &isolate.SandboxConfig{
//	    MemoryLimit:  64 * 1024 * 1024,
//	    Capabilities: []isolate.Capability{isolate.Stdout()},
//	})
package isolate

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/keepalive"
	"google.golang.org/grpc/status"
)

// ClientOption configures the Client. Use the With* functions to create options.
type ClientOption func(*clientOptions)

type clientOptions struct {
	timeout       time.Duration
	tlsEnabled    bool
	tlsConfig     *tls.Config
	rootCAs       *x509.CertPool
	clientCert    *tls.Certificate
	maxRetries    int
	dialOptions   []grpc.DialOption
	keepAlive     *keepalive.ClientParameters
	userAgent     string
	maxMsgSize    int
}

func defaultClientOptions() *clientOptions {
	return &clientOptions{
		timeout:    30 * time.Second,
		maxRetries: 0,
		userAgent:  "isolate-go-sdk/1.0.0",
		maxMsgSize: 64 * 1024 * 1024, // 64MB default max message size
	}
}

// WithTimeout sets the default timeout for all RPC calls. The default is 30 seconds.
// Individual calls can override this by setting a deadline on the context.
func WithTimeout(d time.Duration) ClientOption {
	return func(o *clientOptions) {
		o.timeout = d
	}
}

// WithTLS enables TLS for the gRPC connection. If rootCAs is nil, the system
// certificate pool is used.
func WithTLS(rootCAs *x509.CertPool) ClientOption {
	return func(o *clientOptions) {
		o.tlsEnabled = true
		o.rootCAs = rootCAs
	}
}

// WithMutualTLS enables mutual TLS (mTLS) for the gRPC connection.
func WithMutualTLS(rootCAs *x509.CertPool, clientCert tls.Certificate) ClientOption {
	return func(o *clientOptions) {
		o.tlsEnabled = true
		o.rootCAs = rootCAs
		o.clientCert = &clientCert
	}
}

// WithTLSConfig sets a custom TLS configuration. This overrides WithTLS and
// WithMutualTLS if both are provided.
func WithTLSConfig(cfg *tls.Config) ClientOption {
	return func(o *clientOptions) {
		o.tlsEnabled = true
		o.tlsConfig = cfg
	}
}

// WithRetries sets the maximum number of retries for transient failures.
// The default is 0 (no retries).
func WithRetries(n int) ClientOption {
	return func(o *clientOptions) {
		o.maxRetries = n
	}
}

// WithDialOptions appends additional gRPC dial options to the connection.
func WithDialOptions(opts ...grpc.DialOption) ClientOption {
	return func(o *clientOptions) {
		o.dialOptions = append(o.dialOptions, opts...)
	}
}

// WithKeepAlive configures gRPC keep-alive parameters for the connection.
func WithKeepAlive(params keepalive.ClientParameters) ClientOption {
	return func(o *clientOptions) {
		o.keepAlive = &params
	}
}

// WithUserAgent sets the user-agent string sent with each request.
func WithUserAgent(ua string) ClientOption {
	return func(o *clientOptions) {
		o.userAgent = ua
	}
}

// WithMaxMessageSize sets the maximum message size in bytes for gRPC messages.
// The default is 64MB.
func WithMaxMessageSize(size int) ClientOption {
	return func(o *clientOptions) {
		o.maxMsgSize = size
	}
}

// Client is a client for the Isolate gRPC sandbox service. It is safe for
// concurrent use. Call Close when done to release resources.
type Client struct {
	conn    *grpc.ClientConn
	opts    *clientOptions
	target  string

	mu     sync.RWMutex
	closed bool
}

// NewClient creates a new Client connected to the Isolate gRPC server at the
// given target address (e.g., "localhost:50051").
//
// The caller must call Close when the client is no longer needed.
func NewClient(target string, options ...ClientOption) (*Client, error) {
	opts := defaultClientOptions()
	for _, o := range options {
		o(opts)
	}

	dialOpts := buildDialOptions(opts)

	conn, err := grpc.Dial(target, dialOpts...)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrConnectionFailed, err)
	}

	return &Client{
		conn:   conn,
		opts:   opts,
		target: target,
	}, nil
}

// buildDialOptions constructs the gRPC dial options from the client configuration.
func buildDialOptions(opts *clientOptions) []grpc.DialOption {
	var dialOpts []grpc.DialOption

	// Transport credentials
	if opts.tlsEnabled {
		tlsCfg := opts.tlsConfig
		if tlsCfg == nil {
			tlsCfg = &tls.Config{
				RootCAs: opts.rootCAs,
			}
			if opts.clientCert != nil {
				tlsCfg.Certificates = []tls.Certificate{*opts.clientCert}
			}
		}
		dialOpts = append(dialOpts, grpc.WithTransportCredentials(credentials.NewTLS(tlsCfg)))
	} else {
		dialOpts = append(dialOpts, grpc.WithTransportCredentials(insecure.NewCredentials()))
	}

	// Message size
	if opts.maxMsgSize > 0 {
		dialOpts = append(dialOpts,
			grpc.WithDefaultCallOptions(
				grpc.MaxCallRecvMsgSize(opts.maxMsgSize),
				grpc.MaxCallSendMsgSize(opts.maxMsgSize),
			),
		)
	}

	// Keep-alive
	if opts.keepAlive != nil {
		dialOpts = append(dialOpts, grpc.WithKeepaliveParams(*opts.keepAlive))
	}

	// User-agent
	if opts.userAgent != "" {
		dialOpts = append(dialOpts, grpc.WithUserAgent(opts.userAgent))
	}

	// Additional dial options
	dialOpts = append(dialOpts, opts.dialOptions...)

	return dialOpts
}

// Close closes the gRPC connection and releases all resources. It is safe to
// call Close multiple times.
func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.closed {
		return nil
	}
	c.closed = true
	return c.conn.Close()
}

// ensureOpen returns an error if the client has been closed.
func (c *Client) ensureOpen() error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.closed {
		return ErrClientClosed
	}
	return nil
}

// contextWithTimeout returns a context with the client's default timeout applied,
// unless the provided context already has a deadline.
func (c *Client) contextWithTimeout(ctx context.Context) (context.Context, context.CancelFunc) {
	if _, ok := ctx.Deadline(); ok {
		return ctx, func() {}
	}
	if c.opts.timeout > 0 {
		return context.WithTimeout(ctx, c.opts.timeout)
	}
	return ctx, func() {}
}

// CreateSandbox creates a new sandbox with the given WASM module bytes and
// configuration. If config is nil, server defaults are used.
func (c *Client) CreateSandbox(ctx context.Context, module []byte, config *SandboxConfig) (*CreateSandboxResponse, error) {
	if err := c.ensureOpen(); err != nil {
		return nil, err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	req := marshalCreateSandboxRequest(module, config)

	var resp createSandboxResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/CreateSandbox", req, &resp)
	if err != nil {
		return nil, wrapError("CreateSandbox", "", err)
	}

	return &CreateSandboxResponse{
		SandboxID:      resp.SandboxID,
		ModuleHash:     resp.ModuleHash,
		CreationTimeMs: resp.CreationTimeMs,
	}, nil
}

// RunSandbox runs an existing sandbox identified by sandboxID. If req is nil,
// the sandbox is run with no input and the default entry point.
func (c *Client) RunSandbox(ctx context.Context, sandboxID string, req *RunSandboxRequest) (*RunSandboxResponse, error) {
	if err := c.ensureOpen(); err != nil {
		return nil, err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	protoReq := marshalRunSandboxRequest(sandboxID, req)

	var resp runSandboxResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/RunSandbox", protoReq, &resp)
	if err != nil {
		return nil, wrapError("RunSandbox", sandboxID, err)
	}

	result := &RunSandboxResponse{
		ExitCode:   resp.ExitCode,
		Stdout:     resp.Stdout,
		Stderr:     resp.Stderr,
		DurationMs: resp.DurationMs,
	}
	if resp.ResourceUsage != nil {
		result.ResourceUsage = &ResourceUsage{
			PeakMemory:   resp.ResourceUsage.PeakMemory,
			FuelConsumed: resp.ResourceUsage.FuelConsumed,
			CPUTimeMs:    resp.ResourceUsage.CPUTimeMs,
			WallTimeMs:   resp.ResourceUsage.WallTimeMs,
			BytesRead:    resp.ResourceUsage.BytesRead,
			BytesWritten: resp.ResourceUsage.BytesWritten,
		}
	}

	return result, nil
}

// GetSandbox retrieves information about an existing sandbox.
func (c *Client) GetSandbox(ctx context.Context, sandboxID string) (*SandboxInfo, error) {
	if err := c.ensureOpen(); err != nil {
		return nil, err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	req := &getSandboxRequestProto{SandboxID: sandboxID}

	var resp getSandboxResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/GetSandbox", req, &resp)
	if err != nil {
		return nil, wrapError("GetSandbox", sandboxID, err)
	}

	if resp.Sandbox == nil {
		return nil, wrapError("GetSandbox", sandboxID, fmt.Errorf("%w", ErrSandboxNotFound))
	}

	return unmarshalSandboxInfo(resp.Sandbox), nil
}

// TerminateSandbox terminates an existing sandbox and returns its final metrics.
func (c *Client) TerminateSandbox(ctx context.Context, sandboxID string) (*TerminateSandboxResponse, error) {
	if err := c.ensureOpen(); err != nil {
		return nil, err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	req := &terminateSandboxRequestProto{SandboxID: sandboxID}

	var resp terminateSandboxResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/TerminateSandbox", req, &resp)
	if err != nil {
		return nil, wrapError("TerminateSandbox", sandboxID, err)
	}

	result := &TerminateSandboxResponse{
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

// ListSandboxes returns a list of sandboxes, optionally filtered by state.
// If req is nil, all sandboxes are returned with server-default pagination.
func (c *Client) ListSandboxes(ctx context.Context, req *ListSandboxesRequest) (*ListSandboxesResponse, error) {
	if err := c.ensureOpen(); err != nil {
		return nil, err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	protoReq := marshalListSandboxesRequest(req)

	var resp listSandboxesResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/ListSandboxes", protoReq, &resp)
	if err != nil {
		return nil, wrapError("ListSandboxes", "", err)
	}

	sandboxes := make([]SandboxInfo, 0, len(resp.Sandboxes))
	for _, s := range resp.Sandboxes {
		sandboxes = append(sandboxes, *unmarshalSandboxInfo(s))
	}

	return &ListSandboxesResponse{
		Sandboxes: sandboxes,
		Total:     resp.Total,
	}, nil
}

// GetMetrics retrieves server metrics in the given format ("prometheus" or "json").
func (c *Client) GetMetrics(ctx context.Context, format string) (string, error) {
	if err := c.ensureOpen(); err != nil {
		return "", err
	}

	ctx, cancel := c.contextWithTimeout(ctx)
	defer cancel()

	req := &getMetricsRequestProto{Format: format}

	var resp getMetricsResponseProto
	err := c.invoke(ctx, "/isolate.v1.IsolateService/GetMetrics", req, &resp)
	if err != nil {
		return "", wrapError("GetMetrics", "", err)
	}

	return resp.Data, nil
}

// invoke calls a gRPC method with optional retries.
func (c *Client) invoke(ctx context.Context, method string, req, resp interface{}) error {
	var lastErr error
	attempts := 1 + c.opts.maxRetries

	for i := 0; i < attempts; i++ {
		lastErr = c.conn.Invoke(ctx, method, req, resp)
		if lastErr == nil {
			return nil
		}

		// Only retry on transient errors
		if !isRetryable(lastErr) {
			return lastErr
		}

		// Do not retry if context is done
		if ctx.Err() != nil {
			return lastErr
		}

		// Simple backoff: wait before retrying
		if i < attempts-1 {
			select {
			case <-ctx.Done():
				return lastErr
			case <-time.After(backoffDuration(i)):
			}
		}
	}

	return lastErr
}

// isRetryable returns true if the error is a transient gRPC error worth retrying.
func isRetryable(err error) bool {
	st, ok := status.FromError(err)
	if !ok {
		return false
	}
	switch st.Code() {
	case codes.Unavailable, codes.ResourceExhausted, codes.Aborted:
		return true
	default:
		return false
	}
}

// backoffDuration returns the backoff duration for the given retry attempt.
func backoffDuration(attempt int) time.Duration {
	// Exponential backoff: 100ms, 200ms, 400ms, 800ms, ...
	d := 100 * time.Millisecond
	for i := 0; i < attempt; i++ {
		d *= 2
	}
	if d > 5*time.Second {
		d = 5 * time.Second
	}
	return d
}

// --- Proto marshaling helpers ---
// These types mirror the protobuf messages for use with grpc.Invoke.
// They use protobuf struct tags for proper serialization.

type capabilityProto struct {
	Type  string `protobuf:"bytes,1,opt,name=type,proto3" json:"type,omitempty"`
	Value string `protobuf:"bytes,2,opt,name=value,proto3" json:"value,omitempty"`
}

func (c *capabilityProto) Reset()         {}
func (c *capabilityProto) String() string { return fmt.Sprintf("%s:%s", c.Type, c.Value) }
func (c *capabilityProto) ProtoMessage()  {}

type sandboxConfigProto struct {
	MemoryLimit      uint64             `protobuf:"varint,1,opt,name=memory_limit,json=memoryLimit,proto3" json:"memory_limit,omitempty"`
	FuelLimit        uint64             `protobuf:"varint,2,opt,name=fuel_limit,json=fuelLimit,proto3" json:"fuel_limit,omitempty"`
	WallTimeLimitSecs uint32            `protobuf:"varint,3,opt,name=wall_time_limit_secs,json=wallTimeLimitSecs,proto3" json:"wall_time_limit_secs,omitempty"`
	CPUTimeLimitSecs uint32             `protobuf:"varint,4,opt,name=cpu_time_limit_secs,json=cpuTimeLimitSecs,proto3" json:"cpu_time_limit_secs,omitempty"`
	Capabilities     []*capabilityProto `protobuf:"bytes,5,rep,name=capabilities,proto3" json:"capabilities,omitempty"`
	Env              map[string]string  `protobuf:"bytes,6,rep,name=env,proto3" json:"env,omitempty" protobuf_key:"bytes,1,opt,name=key,proto3" protobuf_val:"bytes,2,opt,name=value,proto3"`
	Args             []string           `protobuf:"bytes,7,rep,name=args,proto3" json:"args,omitempty"`
}

func (s *sandboxConfigProto) Reset()         {}
func (s *sandboxConfigProto) String() string { return "SandboxConfig" }
func (s *sandboxConfigProto) ProtoMessage()  {}

type createSandboxRequestProto struct {
	Module []byte              `protobuf:"bytes,1,opt,name=module,proto3" json:"module,omitempty"`
	Config *sandboxConfigProto `protobuf:"bytes,2,opt,name=config,proto3" json:"config,omitempty"`
}

func (c *createSandboxRequestProto) Reset()         {}
func (c *createSandboxRequestProto) String() string { return "CreateSandboxRequest" }
func (c *createSandboxRequestProto) ProtoMessage()  {}

type createSandboxResponseProto struct {
	SandboxID      string  `protobuf:"bytes,1,opt,name=sandbox_id,json=sandboxId,proto3" json:"sandbox_id,omitempty"`
	ModuleHash     string  `protobuf:"bytes,2,opt,name=module_hash,json=moduleHash,proto3" json:"module_hash,omitempty"`
	CreationTimeMs float64 `protobuf:"fixed64,3,opt,name=creation_time_ms,json=creationTimeMs,proto3" json:"creation_time_ms,omitempty"`
}

func (c *createSandboxResponseProto) Reset()         {}
func (c *createSandboxResponseProto) String() string { return "CreateSandboxResponse" }
func (c *createSandboxResponseProto) ProtoMessage()  {}

type runSandboxRequestProto struct {
	SandboxID  string `protobuf:"bytes,1,opt,name=sandbox_id,json=sandboxId,proto3" json:"sandbox_id,omitempty"`
	Input      []byte `protobuf:"bytes,2,opt,name=input,proto3" json:"input,omitempty"`
	EntryPoint string `protobuf:"bytes,3,opt,name=entry_point,json=entryPoint,proto3" json:"entry_point,omitempty"`
}

func (r *runSandboxRequestProto) Reset()         {}
func (r *runSandboxRequestProto) String() string { return "RunSandboxRequest" }
func (r *runSandboxRequestProto) ProtoMessage()  {}

type resourceUsageProto struct {
	PeakMemory   uint64  `protobuf:"varint,1,opt,name=peak_memory,json=peakMemory,proto3" json:"peak_memory,omitempty"`
	FuelConsumed uint64  `protobuf:"varint,2,opt,name=fuel_consumed,json=fuelConsumed,proto3" json:"fuel_consumed,omitempty"`
	CPUTimeMs    float64 `protobuf:"fixed64,3,opt,name=cpu_time_ms,json=cpuTimeMs,proto3" json:"cpu_time_ms,omitempty"`
	WallTimeMs   float64 `protobuf:"fixed64,4,opt,name=wall_time_ms,json=wallTimeMs,proto3" json:"wall_time_ms,omitempty"`
	BytesRead    uint64  `protobuf:"varint,5,opt,name=bytes_read,json=bytesRead,proto3" json:"bytes_read,omitempty"`
	BytesWritten uint64  `protobuf:"varint,6,opt,name=bytes_written,json=bytesWritten,proto3" json:"bytes_written,omitempty"`
}

func (r *resourceUsageProto) Reset()         {}
func (r *resourceUsageProto) String() string { return "ResourceUsage" }
func (r *resourceUsageProto) ProtoMessage()  {}

type runSandboxResponseProto struct {
	ExitCode      int32               `protobuf:"varint,1,opt,name=exit_code,json=exitCode,proto3" json:"exit_code,omitempty"`
	Stdout        []byte              `protobuf:"bytes,2,opt,name=stdout,proto3" json:"stdout,omitempty"`
	Stderr        []byte              `protobuf:"bytes,3,opt,name=stderr,proto3" json:"stderr,omitempty"`
	DurationMs    float64             `protobuf:"fixed64,4,opt,name=duration_ms,json=durationMs,proto3" json:"duration_ms,omitempty"`
	ResourceUsage *resourceUsageProto `protobuf:"bytes,5,opt,name=resource_usage,json=resourceUsage,proto3" json:"resource_usage,omitempty"`
}

func (r *runSandboxResponseProto) Reset()         {}
func (r *runSandboxResponseProto) String() string { return "RunSandboxResponse" }
func (r *runSandboxResponseProto) ProtoMessage()  {}

type getSandboxRequestProto struct {
	SandboxID string `protobuf:"bytes,1,opt,name=sandbox_id,json=sandboxId,proto3" json:"sandbox_id,omitempty"`
}

func (g *getSandboxRequestProto) Reset()         {}
func (g *getSandboxRequestProto) String() string { return "GetSandboxRequest" }
func (g *getSandboxRequestProto) ProtoMessage()  {}

type sandboxMetricsProto struct {
	RunCount           uint64  `protobuf:"varint,1,opt,name=run_count,json=runCount,proto3" json:"run_count,omitempty"`
	SuccessCount       uint64  `protobuf:"varint,2,opt,name=success_count,json=successCount,proto3" json:"success_count,omitempty"`
	FailureCount       uint64  `protobuf:"varint,3,opt,name=failure_count,json=failureCount,proto3" json:"failure_count,omitempty"`
	TotalRunDurationMs float64 `protobuf:"fixed64,4,opt,name=total_run_duration_ms,json=totalRunDurationMs,proto3" json:"total_run_duration_ms,omitempty"`
	LastRunDurationMs  float64 `protobuf:"fixed64,5,opt,name=last_run_duration_ms,json=lastRunDurationMs,proto3" json:"last_run_duration_ms,omitempty"`
}

func (s *sandboxMetricsProto) Reset()         {}
func (s *sandboxMetricsProto) String() string { return "SandboxMetrics" }
func (s *sandboxMetricsProto) ProtoMessage()  {}

type sandboxInfoProto struct {
	ID         string               `protobuf:"bytes,1,opt,name=id,proto3" json:"id,omitempty"`
	State      string               `protobuf:"bytes,2,opt,name=state,proto3" json:"state,omitempty"`
	ModuleHash string               `protobuf:"bytes,3,opt,name=module_hash,json=moduleHash,proto3" json:"module_hash,omitempty"`
	CreatedAt  int64                `protobuf:"varint,4,opt,name=created_at,json=createdAt,proto3" json:"created_at,omitempty"`
	AgeSecs    float64              `protobuf:"fixed64,5,opt,name=age_secs,json=ageSecs,proto3" json:"age_secs,omitempty"`
	Metrics    *sandboxMetricsProto `protobuf:"bytes,6,opt,name=metrics,proto3" json:"metrics,omitempty"`
}

func (s *sandboxInfoProto) Reset()         {}
func (s *sandboxInfoProto) String() string { return "SandboxInfo" }
func (s *sandboxInfoProto) ProtoMessage()  {}

type getSandboxResponseProto struct {
	Sandbox *sandboxInfoProto `protobuf:"bytes,1,opt,name=sandbox,proto3" json:"sandbox,omitempty"`
}

func (g *getSandboxResponseProto) Reset()         {}
func (g *getSandboxResponseProto) String() string { return "GetSandboxResponse" }
func (g *getSandboxResponseProto) ProtoMessage()  {}

type terminateSandboxRequestProto struct {
	SandboxID string `protobuf:"bytes,1,opt,name=sandbox_id,json=sandboxId,proto3" json:"sandbox_id,omitempty"`
}

func (t *terminateSandboxRequestProto) Reset()         {}
func (t *terminateSandboxRequestProto) String() string { return "TerminateSandboxRequest" }
func (t *terminateSandboxRequestProto) ProtoMessage()  {}

type terminateSandboxResponseProto struct {
	Terminated bool                 `protobuf:"varint,1,opt,name=terminated,proto3" json:"terminated,omitempty"`
	Metrics    *sandboxMetricsProto `protobuf:"bytes,2,opt,name=metrics,proto3" json:"metrics,omitempty"`
}

func (t *terminateSandboxResponseProto) Reset()         {}
func (t *terminateSandboxResponseProto) String() string { return "TerminateSandboxResponse" }
func (t *terminateSandboxResponseProto) ProtoMessage()  {}

type listSandboxesRequestProto struct {
	StateFilter string `protobuf:"bytes,1,opt,name=state_filter,json=stateFilter,proto3" json:"state_filter,omitempty"`
	Limit       int32  `protobuf:"varint,2,opt,name=limit,proto3" json:"limit,omitempty"`
	Offset      int32  `protobuf:"varint,3,opt,name=offset,proto3" json:"offset,omitempty"`
}

func (l *listSandboxesRequestProto) Reset()         {}
func (l *listSandboxesRequestProto) String() string { return "ListSandboxesRequest" }
func (l *listSandboxesRequestProto) ProtoMessage()  {}

type listSandboxesResponseProto struct {
	Sandboxes []*sandboxInfoProto `protobuf:"bytes,1,rep,name=sandboxes,proto3" json:"sandboxes,omitempty"`
	Total     int32               `protobuf:"varint,2,opt,name=total,proto3" json:"total,omitempty"`
}

func (l *listSandboxesResponseProto) Reset()         {}
func (l *listSandboxesResponseProto) String() string { return "ListSandboxesResponse" }
func (l *listSandboxesResponseProto) ProtoMessage()  {}

type getMetricsRequestProto struct {
	Format string `protobuf:"bytes,1,opt,name=format,proto3" json:"format,omitempty"`
}

func (g *getMetricsRequestProto) Reset()         {}
func (g *getMetricsRequestProto) String() string { return "GetMetricsRequest" }
func (g *getMetricsRequestProto) ProtoMessage()  {}

type getMetricsResponseProto struct {
	Data string `protobuf:"bytes,1,opt,name=data,proto3" json:"data,omitempty"`
}

func (g *getMetricsResponseProto) Reset()         {}
func (g *getMetricsResponseProto) String() string { return "GetMetricsResponse" }
func (g *getMetricsResponseProto) ProtoMessage()  {}

// --- Marshaling functions ---

func marshalCreateSandboxRequest(module []byte, config *SandboxConfig) *createSandboxRequestProto {
	req := &createSandboxRequestProto{
		Module: module,
	}

	if config != nil {
		cfg := &sandboxConfigProto{
			MemoryLimit:       config.MemoryLimit,
			FuelLimit:         config.FuelLimit,
			WallTimeLimitSecs: config.WallTimeLimitSecs,
			CPUTimeLimitSecs:  config.CPUTimeLimitSecs,
			Env:               config.Env,
			Args:              config.Args,
		}
		for _, cap := range config.Capabilities {
			cfg.Capabilities = append(cfg.Capabilities, &capabilityProto{
				Type:  cap.Type,
				Value: cap.Value,
			})
		}
		req.Config = cfg
	}

	return req
}

func marshalRunSandboxRequest(sandboxID string, req *RunSandboxRequest) *runSandboxRequestProto {
	protoReq := &runSandboxRequestProto{
		SandboxID: sandboxID,
	}
	if req != nil {
		protoReq.Input = req.Input
		protoReq.EntryPoint = req.EntryPoint
	}
	return protoReq
}

func marshalListSandboxesRequest(req *ListSandboxesRequest) *listSandboxesRequestProto {
	protoReq := &listSandboxesRequestProto{}
	if req != nil {
		protoReq.StateFilter = req.StateFilter
		protoReq.Limit = req.Limit
		protoReq.Offset = req.Offset
	}
	return protoReq
}

func unmarshalSandboxInfo(proto *sandboxInfoProto) *SandboxInfo {
	info := &SandboxInfo{
		ID:         proto.ID,
		State:      proto.State,
		ModuleHash: proto.ModuleHash,
		CreatedAt:  time.Unix(proto.CreatedAt, 0),
		AgeSecs:    proto.AgeSecs,
	}
	if proto.Metrics != nil {
		info.Metrics = &SandboxMetrics{
			RunCount:           proto.Metrics.RunCount,
			SuccessCount:       proto.Metrics.SuccessCount,
			FailureCount:       proto.Metrics.FailureCount,
			TotalRunDurationMs: proto.Metrics.TotalRunDurationMs,
			LastRunDurationMs:  proto.Metrics.LastRunDurationMs,
		}
	}
	return info
}
