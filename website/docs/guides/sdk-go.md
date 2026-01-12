---
sidebar_position: 11
---

# Go SDK

The Go SDK provides a client library for interacting with the Isolate gRPC server from Go applications.

## Installation

```bash
go get github.com/josedab/isolate/sdk/go
```

**Requirements:**
- Go 1.21 or later
- Running Isolate gRPC server

## Quick Start

```go
package main

import (
    "context"
    "fmt"
    "log"
    "os"

    isolate "github.com/josedab/isolate/sdk/go"
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

    // Execute the module
    ctx := context.Background()
    result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
        MemoryLimit:  64 * 1024 * 1024, // 64MB
        Capabilities: []isolate.Capability{isolate.Stdout()},
    })
    if err != nil {
        log.Fatal(err)
    }

    fmt.Printf("Exit code: %d\n", result.ExitCode)
    fmt.Printf("Output: %s\n", string(result.Stdout))
}
```

## Client Creation

### Basic Connection

```go
client, err := isolate.NewClient("localhost:50051")
if err != nil {
    log.Fatal(err)
}
defer client.Close()
```

### With TLS

```go
client, err := isolate.NewClient("isolate.example.com:50051", isolate.ClientOptions{
    TLS: true,
})
```

### With mTLS (Mutual TLS)

```go
// Load CA certificate
caCert, _ := os.ReadFile("ca.crt")
caCertPool := x509.NewCertPool()
caCertPool.AppendCertsFromPEM(caCert)

// Load client certificate
clientCert, _ := tls.LoadX509KeyPair("client.crt", "client.key")

client, err := isolate.NewClient("isolate.example.com:50051", isolate.ClientOptions{
    TLS:        true,
    RootCAs:    caCertPool,
    ClientCert: &clientCert,
})
```

## Sandbox Lifecycle

### One-Shot Execution

The simplest way to run a WASM module. Creates a sandbox, runs it, and terminates it:

```go
result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
    MemoryLimit:  64 * 1024 * 1024,
    FuelLimit:    10_000_000,
    WallTimeLimit: 30, // seconds
    Capabilities: []isolate.Capability{
        isolate.Stdout(),
        isolate.FsRead("/data"),
    },
    Env: map[string]string{
        "CONFIG_PATH": "/etc/app/config.json",
    },
})
```

### Reusable Sandbox

For running the same module multiple times, create a sandbox once and reuse it:

```go
// Create the sandbox
createResult, err := client.CreateSandbox(ctx, wasmBytes, &isolate.CreateSandboxOptions{
    MemoryLimit:  128 * 1024 * 1024,
    Capabilities: []isolate.Capability{isolate.Stdout()},
})
if err != nil {
    log.Fatal(err)
}

sandboxID := createResult.SandboxID
fmt.Printf("Created sandbox: %s\n", sandboxID)

// Run multiple times
for i := 0; i < 5; i++ {
    result, err := client.RunSandbox(ctx, sandboxID, &isolate.RunSandboxOptions{
        Input: []byte(fmt.Sprintf("iteration %d", i)),
    })
    if err != nil {
        log.Printf("Run %d failed: %v", i, err)
        continue
    }
    fmt.Printf("Run %d: exit=%d, duration=%dms\n", i, result.ExitCode, result.DurationMs)
}

// Clean up
terminateResult, err := client.TerminateSandbox(ctx, sandboxID)
if err != nil {
    log.Fatal(err)
}
fmt.Printf("Total runs: %d\n", terminateResult.Metrics.RunCount)
```

## Capabilities

Capabilities control what the WASM module can access. By default, modules have no capabilities.

### Standard I/O

```go
isolate.Stdout()   // Write to stdout
isolate.Stderr()   // Write to stderr
isolate.Stdin()    // Read from stdin
```

### Filesystem

```go
isolate.FsRead("/data")        // Read files under /data
isolate.FsWrite("/tmp/output") // Write files under /tmp/output
isolate.TempDir()              // Access to temporary directory
```

### Network

```go
isolate.HTTP("api.example.com")    // HTTP access to specific host
isolate.HTTP("*.example.com")      // HTTP access with wildcard
isolate.DNS()                      // DNS resolution
```

### Time and Random

```go
isolate.SystemClock()    // Wall clock time
isolate.MonotonicClock() // Monotonic clock for durations
isolate.Random()         // Cryptographic random numbers
```

### Environment Variables

```go
isolate.Env("API_KEY")     // Access to specific env var
isolate.Env("CONFIG_PATH") // Access to another env var
```

### Example: Combining Capabilities

```go
opts := &isolate.ExecuteOptions{
    MemoryLimit: 64 * 1024 * 1024,
    Capabilities: []isolate.Capability{
        isolate.Stdout(),
        isolate.Stderr(),
        isolate.FsRead("/data/input"),
        isolate.FsWrite("/data/output"),
        isolate.HTTP("api.example.com"),
        isolate.SystemClock(),
        isolate.Env("API_KEY"),
    },
    Env: map[string]string{
        "API_KEY": "secret-token",
    },
}
```

## Resource Limits

Control resource consumption to prevent abuse:

```go
opts := &isolate.ExecuteOptions{
    MemoryLimit:   128 * 1024 * 1024, // 128MB heap
    FuelLimit:     50_000_000,        // ~50M instructions
    WallTimeLimit: 60,                // 60 seconds max
    CPUTimeLimit:  30,                // 30 seconds CPU time
}
```

| Option | Description | Default |
|--------|-------------|---------|
| `MemoryLimit` | Maximum heap memory in bytes | Server default |
| `FuelLimit` | Maximum instructions (fuel units) | Unlimited |
| `WallTimeLimit` | Maximum wall-clock time in seconds | Server default |
| `CPUTimeLimit` | Maximum CPU time in seconds | Server default |

## Inspecting Sandboxes

### Get Sandbox Info

```go
info, err := client.GetSandbox(ctx, sandboxID)
if err != nil {
    log.Fatal(err)
}

fmt.Printf("ID: %s\n", info.ID)
fmt.Printf("State: %s\n", info.State)
fmt.Printf("Module Hash: %s\n", info.ModuleHash)
fmt.Printf("Created: %v\n", info.CreatedAt)
fmt.Printf("Age: %d seconds\n", info.AgeSecs)
fmt.Printf("Run Count: %d\n", info.Metrics.RunCount)
```

### List Sandboxes

```go
// List all sandboxes
result, err := client.ListSandboxes(ctx, nil)

// List with filters
result, err := client.ListSandboxes(ctx, &isolate.ListSandboxesOptions{
    StateFilter: "ready",  // Filter by state
    Limit:       10,       // Pagination
    Offset:      0,
})

for _, sandbox := range result.Sandboxes {
    fmt.Printf("%s: %s (%d runs)\n",
        sandbox.ID,
        sandbox.State,
        sandbox.Metrics.RunCount)
}
```

## Metrics

Retrieve server metrics in Prometheus format:

```go
metrics, err := client.GetMetrics(ctx, "prometheus")
if err != nil {
    log.Fatal(err)
}
fmt.Println(metrics)
```

## Error Handling

The SDK wraps gRPC errors with context. Check error types using gRPC status codes:

```go
import "google.golang.org/grpc/status"
import "google.golang.org/grpc/codes"

result, err := client.Execute(ctx, wasmBytes, opts)
if err != nil {
    st, ok := status.FromError(err)
    if ok {
        switch st.Code() {
        case codes.InvalidArgument:
            log.Printf("Invalid input: %s", st.Message())
        case codes.ResourceExhausted:
            log.Printf("Resource limit exceeded: %s", st.Message())
        case codes.DeadlineExceeded:
            log.Printf("Timeout: %s", st.Message())
        case codes.NotFound:
            log.Printf("Sandbox not found: %s", st.Message())
        default:
            log.Printf("Error: %v", err)
        }
    }
}
```

### Common Errors

| gRPC Code | Meaning |
|-----------|---------|
| `InvalidArgument` | Invalid WASM module or configuration |
| `ResourceExhausted` | Memory or fuel limit exceeded |
| `DeadlineExceeded` | Execution timeout |
| `NotFound` | Sandbox ID doesn't exist |
| `PermissionDenied` | Capability not granted |
| `Unavailable` | Server connection issue |

## Complete Example

```go
package main

import (
    "context"
    "fmt"
    "log"
    "os"
    "time"

    isolate "github.com/josedab/isolate/sdk/go"
)

func main() {
    // Connect
    client, err := isolate.NewClient("localhost:50051")
    if err != nil {
        log.Fatalf("Failed to connect: %v", err)
    }
    defer client.Close()

    // Load module
    wasmBytes, err := os.ReadFile("processor.wasm")
    if err != nil {
        log.Fatalf("Failed to load module: %v", err)
    }

    // Create context with timeout
    ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
    defer cancel()

    // Execute with full configuration
    result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
        MemoryLimit:   64 * 1024 * 1024,
        FuelLimit:     10_000_000,
        WallTimeLimit: 30,
        Capabilities: []isolate.Capability{
            isolate.Stdout(),
            isolate.Stderr(),
            isolate.FsRead("/data"),
            isolate.SystemClock(),
        },
        Args: []string{"--verbose", "--format=json"},
        Env: map[string]string{
            "LOG_LEVEL": "debug",
        },
    })
    if err != nil {
        log.Fatalf("Execution failed: %v", err)
    }

    // Process results
    fmt.Printf("Exit Code: %d\n", result.ExitCode)
    fmt.Printf("Duration: %dms\n", result.DurationMs)
    fmt.Printf("Memory Peak: %d bytes\n", result.ResourceUsage.PeakMemory)
    fmt.Printf("Fuel Used: %d\n", result.ResourceUsage.FuelConsumed)

    if result.ExitCode == 0 {
        fmt.Printf("Output:\n%s\n", string(result.Stdout))
    } else {
        fmt.Printf("Error:\n%s\n", string(result.Stderr))
    }
}
```

## See Also

- [gRPC Server](./grpc-server) - Running the Isolate server
- [TypeScript SDK](./sdk-typescript) - TypeScript/Node.js client
- [API Reference](../reference/api) - Core API documentation
