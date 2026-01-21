# Isolate Go SDK

A Go client library for the [Isolate](https://github.com/josedab/isolate) gRPC sandbox service. Isolate provides secure execution of untrusted WebAssembly (WASM) modules with capability-based security and resource controls.

## Installation

```bash
go get github.com/josedab/isolate/sdk/go
```

**Requirements:**
- Go 1.21 or later
- A running Isolate gRPC server

## Quick Start

```go
package main

import (
    "context"
    "fmt"
    "log"
    "os"

    "github.com/josedab/isolate/sdk/go/isolate"
)

func main() {
    // Connect to the Isolate server
    client, err := isolate.NewClient("localhost:50051")
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    // Load a WASM module
    wasmBytes, err := os.ReadFile("module.wasm")
    if err != nil {
        log.Fatal(err)
    }

    ctx := context.Background()

    // Create a sandbox
    createResp, err := client.CreateSandbox(ctx, wasmBytes, &isolate.SandboxConfig{
        MemoryLimit:  64 * 1024 * 1024, // 64MB
        FuelLimit:    10_000_000,
        Capabilities: []isolate.Capability{isolate.Stdout()},
    })
    if err != nil {
        log.Fatal(err)
    }
    fmt.Printf("Created sandbox: %s\n", createResp.SandboxID)

    // Run the sandbox
    runResp, err := client.RunSandbox(ctx, createResp.SandboxID, nil)
    if err != nil {
        log.Fatal(err)
    }

    fmt.Printf("Exit code: %d\n", runResp.ExitCode)
    fmt.Printf("Output: %s\n", string(runResp.Stdout))
}
```

## Client Options

The client uses the functional options pattern for configuration:

```go
client, err := isolate.NewClient("localhost:50051",
    isolate.WithTimeout(10 * time.Second),   // Default RPC timeout
    isolate.WithRetries(3),                   // Retry transient failures
    isolate.WithTLS(nil),                     // Enable TLS with system CA pool
    isolate.WithUserAgent("my-app/1.0"),      // Custom user-agent
    isolate.WithMaxMessageSize(128*1024*1024), // 128MB max message
)
```

### Available Options

| Option | Description | Default |
|--------|-------------|---------|
| `WithTimeout(d)` | Default timeout for RPC calls | 30 seconds |
| `WithRetries(n)` | Max retries for transient failures | 0 (no retries) |
| `WithTLS(rootCAs)` | Enable TLS (nil uses system CAs) | Disabled |
| `WithMutualTLS(rootCAs, cert)` | Enable mutual TLS | Disabled |
| `WithTLSConfig(cfg)` | Custom TLS configuration | None |
| `WithKeepAlive(params)` | gRPC keep-alive parameters | None |
| `WithUserAgent(ua)` | User-agent string | `isolate-go-sdk/1.0.0` |
| `WithMaxMessageSize(n)` | Max gRPC message size in bytes | 64MB |
| `WithDialOptions(opts...)` | Additional gRPC dial options | None |

## API Methods

All methods accept `context.Context` as the first parameter. If the context does not have a deadline, the client's default timeout is applied.

### CreateSandbox

Create a new sandbox with a WASM module and configuration:

```go
resp, err := client.CreateSandbox(ctx, wasmBytes, &isolate.SandboxConfig{
    MemoryLimit:       128 * 1024 * 1024,
    FuelLimit:         50_000_000,
    WallTimeLimitSecs: 60,
    CPUTimeLimitSecs:  30,
    Capabilities: []isolate.Capability{
        isolate.Stdout(),
        isolate.Stderr(),
        isolate.FsRead("/data"),
        isolate.HTTP("api.example.com"),
    },
    Env:  map[string]string{"LOG_LEVEL": "debug"},
    Args: []string{"--verbose"},
})
```

### RunSandbox

Run an existing sandbox:

```go
resp, err := client.RunSandbox(ctx, sandboxID, &isolate.RunSandboxRequest{
    Input:      []byte("input data"),
    EntryPoint: "_start", // default if empty
})

fmt.Printf("Exit: %d, Duration: %.2fms\n", resp.ExitCode, resp.DurationMs)
fmt.Printf("Memory: %d bytes, Fuel: %d\n",
    resp.ResourceUsage.PeakMemory, resp.ResourceUsage.FuelConsumed)
```

### GetSandbox

Retrieve sandbox information:

```go
info, err := client.GetSandbox(ctx, sandboxID)
fmt.Printf("State: %s, Runs: %d\n", info.State, info.Metrics.RunCount)
```

### TerminateSandbox

Terminate a sandbox and get final metrics:

```go
resp, err := client.TerminateSandbox(ctx, sandboxID)
if resp.Terminated {
    fmt.Printf("Total runs: %d\n", resp.Metrics.RunCount)
}
```

### ListSandboxes

List sandboxes with optional filtering and pagination:

```go
resp, err := client.ListSandboxes(ctx, &isolate.ListSandboxesRequest{
    StateFilter: "ready",
    Limit:       10,
    Offset:      0,
})

for _, sb := range resp.Sandboxes {
    fmt.Printf("%s: %s\n", sb.ID, sb.State)
}
```

### GetMetrics

Retrieve server metrics:

```go
data, err := client.GetMetrics(ctx, "prometheus")
fmt.Println(data)
```

## Capabilities

Capabilities define what the WASM module is allowed to access. Modules have no capabilities by default.

```go
// Standard I/O
isolate.Stdout()
isolate.Stderr()
isolate.Stdin()

// Filesystem
isolate.FsRead("/path")
isolate.FsWrite("/path")
isolate.TempDir()

// Network
isolate.HTTP("host.example.com")
isolate.DNS()

// System
isolate.SystemClock()
isolate.MonotonicClock()
isolate.Random()

// Environment
isolate.EnvVar("VAR_NAME")
```

## Error Handling

The SDK provides custom error types with proper wrapping. Errors can be inspected with `errors.Is` and `errors.As`:

```go
resp, err := client.RunSandbox(ctx, sandboxID, nil)
if err != nil {
    // Check for specific error types
    if isolate.IsNotFound(err) {
        log.Printf("Sandbox not found")
    } else if isolate.IsResourceExhausted(err) {
        log.Printf("Resource limit exceeded")
    } else if isolate.IsDeadlineExceeded(err) {
        log.Printf("Execution timed out")
    } else if isolate.IsPermissionDenied(err) {
        log.Printf("Missing capability")
    } else if isolate.IsUnavailable(err) {
        log.Printf("Server unreachable")
    } else {
        log.Printf("Error: %v", err)
    }

    // Access error details
    var ie *isolate.IsolateError
    if errors.As(err, &ie) {
        log.Printf("Operation: %s, Sandbox: %s, Code: %v",
            ie.Op, ie.SandboxID, ie.Code)
    }
}
```

### Sentinel Errors

| Error | Description |
|-------|-------------|
| `ErrConnectionFailed` | Could not connect to the server |
| `ErrClientClosed` | Client has already been closed |
| `ErrSandboxNotFound` | Sandbox ID does not exist |
| `ErrInvalidArgument` | Invalid WASM module or configuration |
| `ErrResourceExhausted` | Memory, fuel, or time limit exceeded |
| `ErrDeadlineExceeded` | Operation timed out |
| `ErrPermissionDenied` | Required capability not granted |
| `ErrUnavailable` | Server is unreachable |

## See Also

- [gRPC Server Documentation](../../website/docs/guides/grpc-server.md) - Running the Isolate server
- [TypeScript SDK](../../website/docs/guides/sdk-typescript.md) - TypeScript/Node.js client
- [Proto Definition](../../proto/isolate.proto) - gRPC service definition
