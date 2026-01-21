package isolate

import (
	"context"
	"errors"
	"testing"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// --- Error tests ---

func TestWrapError_NilError(t *testing.T) {
	err := wrapError("TestOp", "", nil)
	if err != nil {
		t.Fatalf("expected nil error, got: %v", err)
	}
}

func TestWrapError_NotFound(t *testing.T) {
	grpcErr := status.Error(codes.NotFound, "sandbox abc123 not found")
	err := wrapError("GetSandbox", "abc123", grpcErr)

	if err == nil {
		t.Fatal("expected error, got nil")
	}

	var ie *IsolateError
	if !errors.As(err, &ie) {
		t.Fatalf("expected IsolateError, got %T", err)
	}
	if ie.Op != "GetSandbox" {
		t.Errorf("expected op GetSandbox, got %s", ie.Op)
	}
	if ie.SandboxID != "abc123" {
		t.Errorf("expected sandboxID abc123, got %s", ie.SandboxID)
	}
	if ie.Code != codes.NotFound {
		t.Errorf("expected code NotFound, got %v", ie.Code)
	}
	if !errors.Is(err, ErrSandboxNotFound) {
		t.Error("expected error to wrap ErrSandboxNotFound")
	}
	if !IsNotFound(err) {
		t.Error("expected IsNotFound to return true")
	}
}

func TestWrapError_InvalidArgument(t *testing.T) {
	grpcErr := status.Error(codes.InvalidArgument, "invalid module")
	err := wrapError("CreateSandbox", "", grpcErr)

	if !IsInvalidArgument(err) {
		t.Error("expected IsInvalidArgument to return true")
	}

	var ie *IsolateError
	if !errors.As(err, &ie) {
		t.Fatalf("expected IsolateError, got %T", err)
	}
	if ie.Code != codes.InvalidArgument {
		t.Errorf("expected code InvalidArgument, got %v", ie.Code)
	}
}

func TestWrapError_ResourceExhausted(t *testing.T) {
	grpcErr := status.Error(codes.ResourceExhausted, "memory limit exceeded")
	err := wrapError("RunSandbox", "sb-1", grpcErr)

	if !IsResourceExhausted(err) {
		t.Error("expected IsResourceExhausted to return true")
	}
}

func TestWrapError_DeadlineExceeded(t *testing.T) {
	grpcErr := status.Error(codes.DeadlineExceeded, "timeout")
	err := wrapError("RunSandbox", "sb-1", grpcErr)

	if !IsDeadlineExceeded(err) {
		t.Error("expected IsDeadlineExceeded to return true")
	}
}

func TestWrapError_PermissionDenied(t *testing.T) {
	grpcErr := status.Error(codes.PermissionDenied, "stdout not granted")
	err := wrapError("RunSandbox", "sb-1", grpcErr)

	if !IsPermissionDenied(err) {
		t.Error("expected IsPermissionDenied to return true")
	}
}

func TestWrapError_Unavailable(t *testing.T) {
	grpcErr := status.Error(codes.Unavailable, "connection refused")
	err := wrapError("CreateSandbox", "", grpcErr)

	if !IsUnavailable(err) {
		t.Error("expected IsUnavailable to return true")
	}
}

func TestWrapError_UnknownCode(t *testing.T) {
	grpcErr := status.Error(codes.Internal, "internal error")
	err := wrapError("RunSandbox", "sb-1", grpcErr)

	if err == nil {
		t.Fatal("expected error, got nil")
	}

	var ie *IsolateError
	if !errors.As(err, &ie) {
		t.Fatalf("expected IsolateError, got %T", err)
	}
	if ie.Code != codes.Internal {
		t.Errorf("expected code Internal, got %v", ie.Code)
	}
	// Should not match any sentinel error
	if IsNotFound(err) || IsInvalidArgument(err) || IsResourceExhausted(err) ||
		IsDeadlineExceeded(err) || IsPermissionDenied(err) || IsUnavailable(err) {
		t.Error("expected no sentinel error match for Internal code")
	}
}

func TestIsolateError_ErrorString(t *testing.T) {
	tests := []struct {
		name      string
		err       *IsolateError
		wantContains string
	}{
		{
			name: "with sandbox ID",
			err: &IsolateError{
				Op:        "RunSandbox",
				SandboxID: "abc123",
				Err:       errors.New("something failed"),
			},
			wantContains: "sandbox=abc123",
		},
		{
			name: "without sandbox ID",
			err: &IsolateError{
				Op:  "ListSandboxes",
				Err: errors.New("something failed"),
			},
			wantContains: "ListSandboxes",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			msg := tt.err.Error()
			if len(msg) == 0 {
				t.Error("expected non-empty error string")
			}
			if !containsString(msg, tt.wantContains) {
				t.Errorf("error string %q does not contain %q", msg, tt.wantContains)
			}
		})
	}
}

func TestIsolateError_Unwrap(t *testing.T) {
	inner := errors.New("inner error")
	ie := &IsolateError{
		Op:  "test",
		Err: inner,
	}
	if !errors.Is(ie, inner) {
		t.Error("expected Unwrap to return inner error")
	}
}

// --- Model tests ---

func TestCapabilityConstructors(t *testing.T) {
	tests := []struct {
		name      string
		cap       Capability
		wantType  string
		wantValue string
	}{
		{"Stdout", Stdout(), "stdout", ""},
		{"Stderr", Stderr(), "stderr", ""},
		{"Stdin", Stdin(), "stdin", ""},
		{"FsRead", FsRead("/data"), "fs_read", "/data"},
		{"FsWrite", FsWrite("/tmp"), "fs_write", "/tmp"},
		{"TempDir", TempDir(), "temp_dir", ""},
		{"HTTP", HTTP("api.example.com"), "http", "api.example.com"},
		{"DNS", DNS(), "dns", ""},
		{"SystemClock", SystemClock(), "system_clock", ""},
		{"MonotonicClock", MonotonicClock(), "monotonic_clock", ""},
		{"Random", Random(), "random", ""},
		{"EnvVar", EnvVar("API_KEY"), "env", "API_KEY"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if tt.cap.Type != tt.wantType {
				t.Errorf("expected type %q, got %q", tt.wantType, tt.cap.Type)
			}
			if tt.cap.Value != tt.wantValue {
				t.Errorf("expected value %q, got %q", tt.wantValue, tt.cap.Value)
			}
		})
	}
}

// --- Client option tests ---

func TestDefaultClientOptions(t *testing.T) {
	opts := defaultClientOptions()

	if opts.timeout != 30*time.Second {
		t.Errorf("expected default timeout 30s, got %v", opts.timeout)
	}
	if opts.maxRetries != 0 {
		t.Errorf("expected default maxRetries 0, got %d", opts.maxRetries)
	}
	if opts.tlsEnabled {
		t.Error("expected TLS disabled by default")
	}
	if opts.maxMsgSize != 64*1024*1024 {
		t.Errorf("expected default maxMsgSize 64MB, got %d", opts.maxMsgSize)
	}
	if opts.userAgent != "isolate-go-sdk/1.0.0" {
		t.Errorf("expected default user-agent, got %q", opts.userAgent)
	}
}

func TestWithTimeout(t *testing.T) {
	opts := defaultClientOptions()
	WithTimeout(10 * time.Second)(opts)

	if opts.timeout != 10*time.Second {
		t.Errorf("expected timeout 10s, got %v", opts.timeout)
	}
}

func TestWithRetries(t *testing.T) {
	opts := defaultClientOptions()
	WithRetries(3)(opts)

	if opts.maxRetries != 3 {
		t.Errorf("expected maxRetries 3, got %d", opts.maxRetries)
	}
}

func TestWithTLS(t *testing.T) {
	opts := defaultClientOptions()
	WithTLS(nil)(opts)

	if !opts.tlsEnabled {
		t.Error("expected TLS to be enabled")
	}
	if opts.rootCAs != nil {
		t.Error("expected nil rootCAs when none provided")
	}
}

func TestWithUserAgent(t *testing.T) {
	opts := defaultClientOptions()
	WithUserAgent("custom-agent/2.0")(opts)

	if opts.userAgent != "custom-agent/2.0" {
		t.Errorf("expected user-agent %q, got %q", "custom-agent/2.0", opts.userAgent)
	}
}

func TestWithMaxMessageSize(t *testing.T) {
	opts := defaultClientOptions()
	WithMaxMessageSize(128 * 1024 * 1024)(opts)

	if opts.maxMsgSize != 128*1024*1024 {
		t.Errorf("expected maxMsgSize 128MB, got %d", opts.maxMsgSize)
	}
}

// --- Client lifecycle tests ---

func TestNewClient(t *testing.T) {
	client, err := NewClient("localhost:50051")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	defer client.Close()

	if client.target != "localhost:50051" {
		t.Errorf("expected target localhost:50051, got %s", client.target)
	}
	if client.conn == nil {
		t.Error("expected non-nil connection")
	}
	if client.closed {
		t.Error("expected client to not be closed")
	}
}

func TestNewClient_WithOptions(t *testing.T) {
	client, err := NewClient("localhost:50051",
		WithTimeout(5*time.Second),
		WithRetries(2),
		WithUserAgent("test-agent/1.0"),
		WithMaxMessageSize(32*1024*1024),
	)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	defer client.Close()

	if client.opts.timeout != 5*time.Second {
		t.Errorf("expected timeout 5s, got %v", client.opts.timeout)
	}
	if client.opts.maxRetries != 2 {
		t.Errorf("expected maxRetries 2, got %d", client.opts.maxRetries)
	}
}

func TestClient_CloseIdempotent(t *testing.T) {
	client, err := NewClient("localhost:50051")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	// First close should succeed
	if err := client.Close(); err != nil {
		t.Fatalf("unexpected error on first close: %v", err)
	}

	// Second close should also succeed (idempotent)
	if err := client.Close(); err != nil {
		t.Fatalf("unexpected error on second close: %v", err)
	}
}

func TestClient_EnsureOpenAfterClose(t *testing.T) {
	client, err := NewClient("localhost:50051")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	client.Close()

	if err := client.ensureOpen(); !errors.Is(err, ErrClientClosed) {
		t.Errorf("expected ErrClientClosed, got %v", err)
	}
}

func TestClient_MethodsFailAfterClose(t *testing.T) {
	client, err := NewClient("localhost:50051")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	client.Close()

	ctx := context.Background()

	_, err = client.CreateSandbox(ctx, []byte("test"), nil)
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("CreateSandbox: expected ErrClientClosed, got %v", err)
	}

	_, err = client.RunSandbox(ctx, "sb-1", nil)
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("RunSandbox: expected ErrClientClosed, got %v", err)
	}

	_, err = client.GetSandbox(ctx, "sb-1")
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("GetSandbox: expected ErrClientClosed, got %v", err)
	}

	_, err = client.TerminateSandbox(ctx, "sb-1")
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("TerminateSandbox: expected ErrClientClosed, got %v", err)
	}

	_, err = client.ListSandboxes(ctx, nil)
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("ListSandboxes: expected ErrClientClosed, got %v", err)
	}

	_, err = client.GetMetrics(ctx, "prometheus")
	if !errors.Is(err, ErrClientClosed) {
		t.Errorf("GetMetrics: expected ErrClientClosed, got %v", err)
	}
}

// --- Context timeout tests ---

func TestClient_ContextWithTimeout_UsesDefaultWhenNoDeadline(t *testing.T) {
	client, err := NewClient("localhost:50051", WithTimeout(5*time.Second))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	defer client.Close()

	ctx := context.Background()
	newCtx, cancel := client.contextWithTimeout(ctx)
	defer cancel()

	deadline, ok := newCtx.Deadline()
	if !ok {
		t.Fatal("expected deadline to be set")
	}

	remaining := time.Until(deadline)
	if remaining < 4*time.Second || remaining > 6*time.Second {
		t.Errorf("expected deadline ~5s from now, got %v", remaining)
	}
}

func TestClient_ContextWithTimeout_RespectsExistingDeadline(t *testing.T) {
	client, err := NewClient("localhost:50051", WithTimeout(30*time.Second))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	defer client.Close()

	// Create context with a shorter deadline
	ctx, ctxCancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer ctxCancel()

	newCtx, cancel := client.contextWithTimeout(ctx)
	defer cancel()

	deadline, ok := newCtx.Deadline()
	if !ok {
		t.Fatal("expected deadline to be set")
	}

	remaining := time.Until(deadline)
	// Should use the existing 2s deadline, not the client's 30s default
	if remaining > 3*time.Second {
		t.Errorf("expected deadline ~2s from now, got %v", remaining)
	}
}

// --- Marshaling tests ---

func TestMarshalCreateSandboxRequest_NilConfig(t *testing.T) {
	req := marshalCreateSandboxRequest([]byte("wasm"), nil)

	if string(req.Module) != "wasm" {
		t.Errorf("expected module bytes 'wasm', got %q", req.Module)
	}
	if req.Config != nil {
		t.Error("expected nil config")
	}
}

func TestMarshalCreateSandboxRequest_WithConfig(t *testing.T) {
	config := &SandboxConfig{
		MemoryLimit:       64 * 1024 * 1024,
		FuelLimit:         1_000_000,
		WallTimeLimitSecs: 30,
		CPUTimeLimitSecs:  10,
		Capabilities:      []Capability{Stdout(), FsRead("/data")},
		Env:               map[string]string{"KEY": "value"},
		Args:              []string{"--verbose"},
	}
	req := marshalCreateSandboxRequest([]byte("wasm"), config)

	if req.Config == nil {
		t.Fatal("expected non-nil config")
	}
	if req.Config.MemoryLimit != 64*1024*1024 {
		t.Errorf("expected memory limit 64MB, got %d", req.Config.MemoryLimit)
	}
	if req.Config.FuelLimit != 1_000_000 {
		t.Errorf("expected fuel limit 1000000, got %d", req.Config.FuelLimit)
	}
	if req.Config.WallTimeLimitSecs != 30 {
		t.Errorf("expected wall time limit 30, got %d", req.Config.WallTimeLimitSecs)
	}
	if req.Config.CPUTimeLimitSecs != 10 {
		t.Errorf("expected cpu time limit 10, got %d", req.Config.CPUTimeLimitSecs)
	}
	if len(req.Config.Capabilities) != 2 {
		t.Fatalf("expected 2 capabilities, got %d", len(req.Config.Capabilities))
	}
	if req.Config.Capabilities[0].Type != "stdout" {
		t.Errorf("expected first capability type stdout, got %s", req.Config.Capabilities[0].Type)
	}
	if req.Config.Capabilities[1].Type != "fs_read" {
		t.Errorf("expected second capability type fs_read, got %s", req.Config.Capabilities[1].Type)
	}
	if req.Config.Capabilities[1].Value != "/data" {
		t.Errorf("expected second capability value /data, got %s", req.Config.Capabilities[1].Value)
	}
	if req.Config.Env["KEY"] != "value" {
		t.Errorf("expected env KEY=value, got %s", req.Config.Env["KEY"])
	}
	if len(req.Config.Args) != 1 || req.Config.Args[0] != "--verbose" {
		t.Errorf("expected args [--verbose], got %v", req.Config.Args)
	}
}

func TestMarshalRunSandboxRequest_NilRequest(t *testing.T) {
	req := marshalRunSandboxRequest("sb-1", nil)

	if req.SandboxID != "sb-1" {
		t.Errorf("expected sandbox ID sb-1, got %s", req.SandboxID)
	}
	if req.Input != nil {
		t.Error("expected nil input")
	}
	if req.EntryPoint != "" {
		t.Errorf("expected empty entry point, got %s", req.EntryPoint)
	}
}

func TestMarshalRunSandboxRequest_WithRequest(t *testing.T) {
	runReq := &RunSandboxRequest{
		Input:      []byte("hello"),
		EntryPoint: "main",
	}
	req := marshalRunSandboxRequest("sb-1", runReq)

	if string(req.Input) != "hello" {
		t.Errorf("expected input 'hello', got %q", req.Input)
	}
	if req.EntryPoint != "main" {
		t.Errorf("expected entry point 'main', got %s", req.EntryPoint)
	}
}

func TestMarshalListSandboxesRequest_NilRequest(t *testing.T) {
	req := marshalListSandboxesRequest(nil)

	if req.StateFilter != "" {
		t.Errorf("expected empty state filter, got %s", req.StateFilter)
	}
	if req.Limit != 0 {
		t.Errorf("expected limit 0, got %d", req.Limit)
	}
	if req.Offset != 0 {
		t.Errorf("expected offset 0, got %d", req.Offset)
	}
}

func TestMarshalListSandboxesRequest_WithRequest(t *testing.T) {
	listReq := &ListSandboxesRequest{
		StateFilter: "ready",
		Limit:       10,
		Offset:      20,
	}
	req := marshalListSandboxesRequest(listReq)

	if req.StateFilter != "ready" {
		t.Errorf("expected state filter 'ready', got %s", req.StateFilter)
	}
	if req.Limit != 10 {
		t.Errorf("expected limit 10, got %d", req.Limit)
	}
	if req.Offset != 20 {
		t.Errorf("expected offset 20, got %d", req.Offset)
	}
}

// --- Unmarshal tests ---

func TestUnmarshalSandboxInfo_WithMetrics(t *testing.T) {
	proto := &sandboxInfoProto{
		ID:         "sb-123",
		State:      "ready",
		ModuleHash: "abc",
		CreatedAt:  1700000000,
		AgeSecs:    42.5,
		Metrics: &sandboxMetricsProto{
			RunCount:           10,
			SuccessCount:       8,
			FailureCount:       2,
			TotalRunDurationMs: 5000.0,
			LastRunDurationMs:  250.0,
		},
	}

	info := unmarshalSandboxInfo(proto)

	if info.ID != "sb-123" {
		t.Errorf("expected ID sb-123, got %s", info.ID)
	}
	if info.State != "ready" {
		t.Errorf("expected state ready, got %s", info.State)
	}
	if info.ModuleHash != "abc" {
		t.Errorf("expected module hash abc, got %s", info.ModuleHash)
	}
	expectedTime := time.Unix(1700000000, 0)
	if !info.CreatedAt.Equal(expectedTime) {
		t.Errorf("expected created at %v, got %v", expectedTime, info.CreatedAt)
	}
	if info.AgeSecs != 42.5 {
		t.Errorf("expected age 42.5, got %f", info.AgeSecs)
	}
	if info.Metrics == nil {
		t.Fatal("expected non-nil metrics")
	}
	if info.Metrics.RunCount != 10 {
		t.Errorf("expected run count 10, got %d", info.Metrics.RunCount)
	}
	if info.Metrics.SuccessCount != 8 {
		t.Errorf("expected success count 8, got %d", info.Metrics.SuccessCount)
	}
	if info.Metrics.FailureCount != 2 {
		t.Errorf("expected failure count 2, got %d", info.Metrics.FailureCount)
	}
	if info.Metrics.TotalRunDurationMs != 5000.0 {
		t.Errorf("expected total run duration 5000, got %f", info.Metrics.TotalRunDurationMs)
	}
	if info.Metrics.LastRunDurationMs != 250.0 {
		t.Errorf("expected last run duration 250, got %f", info.Metrics.LastRunDurationMs)
	}
}

func TestUnmarshalSandboxInfo_NilMetrics(t *testing.T) {
	proto := &sandboxInfoProto{
		ID:    "sb-456",
		State: "terminated",
	}

	info := unmarshalSandboxInfo(proto)

	if info.Metrics != nil {
		t.Error("expected nil metrics")
	}
}

// --- Retry helper tests ---

func TestIsRetryable(t *testing.T) {
	tests := []struct {
		code     codes.Code
		expected bool
	}{
		{codes.Unavailable, true},
		{codes.ResourceExhausted, true},
		{codes.Aborted, true},
		{codes.NotFound, false},
		{codes.InvalidArgument, false},
		{codes.Internal, false},
		{codes.PermissionDenied, false},
		{codes.DeadlineExceeded, false},
	}

	for _, tt := range tests {
		t.Run(tt.code.String(), func(t *testing.T) {
			err := status.Error(tt.code, "test error")
			if got := isRetryable(err); got != tt.expected {
				t.Errorf("isRetryable(%v) = %v, want %v", tt.code, got, tt.expected)
			}
		})
	}
}

func TestIsRetryable_NonGRPCError(t *testing.T) {
	err := errors.New("plain error")
	if isRetryable(err) {
		t.Error("expected non-gRPC error to not be retryable")
	}
}

func TestBackoffDuration(t *testing.T) {
	tests := []struct {
		attempt  int
		expected time.Duration
	}{
		{0, 100 * time.Millisecond},
		{1, 200 * time.Millisecond},
		{2, 400 * time.Millisecond},
		{3, 800 * time.Millisecond},
		{4, 1600 * time.Millisecond},
		{5, 3200 * time.Millisecond},
		{6, 5 * time.Second}, // capped
		{10, 5 * time.Second}, // capped
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := backoffDuration(tt.attempt)
			if got != tt.expected {
				t.Errorf("backoffDuration(%d) = %v, want %v", tt.attempt, got, tt.expected)
			}
		})
	}
}

// --- Build dial options tests ---

func TestBuildDialOptions_Insecure(t *testing.T) {
	opts := defaultClientOptions()
	dialOpts := buildDialOptions(opts)

	if len(dialOpts) == 0 {
		t.Fatal("expected at least one dial option")
	}
}

func TestBuildDialOptions_TLS(t *testing.T) {
	opts := defaultClientOptions()
	opts.tlsEnabled = true
	dialOpts := buildDialOptions(opts)

	if len(dialOpts) == 0 {
		t.Fatal("expected at least one dial option")
	}
}

// --- Helper ---

func containsString(s, substr string) bool {
	return len(s) >= len(substr) && searchString(s, substr)
}

func searchString(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
