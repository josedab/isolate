# Isolate Go SDK

Go client SDK for [Isolate](https://github.com/josedab/isolate), a secure sandbox runtime for WebAssembly.

## Installation

```bash
go get github.com/josedab/isolate/sdk/go
```

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
	// Create a client
	client, err := isolate.NewClient("localhost:50051")
	if err != nil {
		log.Fatal(err)
	}
	defer client.Close()

	// Load your WASM module
	wasmBytes, err := os.ReadFile("module.wasm")
	if err != nil {
		log.Fatal(err)
	}

	// Create and run a sandbox
	ctx := context.Background()
	result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
		MemoryLimit: 64 * 1024 * 1024, // 64MB
		FuelLimit:   10_000_000,       // 10M instructions
		Capabilities: []isolate.Capability{
			isolate.Stdout(),
			isolate.Stderr(),
		},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Exit code: %d\n", result.ExitCode)
	fmt.Printf("Stdout: %s\n", string(result.Stdout))
}
```

## API Reference

### Client

The main client struct for interacting with the Isolate server.

#### NewClient

```go
client, err := isolate.NewClient(address string, opts ...ClientOptions)
```

- `address` - Server address (e.g., `"localhost:50051"`)
- `opts` - Optional configuration
  - `TLS` - Enable TLS connection
  - `RootCAs` - Root certificate pool for TLS
  - `ClientCert` - Client certificate for mTLS
  - `DialTimeout` - Connection timeout

#### Methods

##### CreateSandbox

Create a new sandbox with the given WASM module.

```go
result, err := client.CreateSandbox(ctx, wasmBytes, &isolate.CreateSandboxOptions{
	MemoryLimit:       64 * 1024 * 1024,
	FuelLimit:         10_000_000,
	WallTimeLimitSecs: 30,
	Capabilities:      []isolate.Capability{isolate.Stdout()},
	Env:               map[string]string{"API_KEY": "secret"},
	Args:              []string{"--verbose"},
})

fmt.Printf("Sandbox ID: %s\n", result.SandboxID)
fmt.Printf("Module Hash: %s\n", result.ModuleHash)
```

##### RunSandbox

Run an existing sandbox.

```go
result, err := client.RunSandbox(ctx, sandboxID, &isolate.RunSandboxOptions{
	Input:      []byte("Hello"),
	EntryPoint: "_start",
})

fmt.Printf("Exit code: %d\n", result.ExitCode)
fmt.Printf("Stdout: %s\n", string(result.Stdout))
fmt.Printf("Peak memory: %d\n", result.ResourceUsage.PeakMemory)
```

##### Execute

Convenience method that creates, runs, and terminates a sandbox in one call.

```go
result, err := client.Execute(ctx, wasmBytes, &isolate.ExecuteOptions{
	Capabilities: []isolate.Capability{isolate.Stdout()},
})
```

##### GetSandbox

Get sandbox status and metrics.

```go
info, err := client.GetSandbox(ctx, sandboxID)
fmt.Printf("State: %s, Metrics: %+v\n", info.State, info.Metrics)
```

##### ListSandboxes

List all sandboxes.

```go
result, err := client.ListSandboxes(ctx, &isolate.ListSandboxesOptions{
	StateFilter: "ready",
	Limit:       10,
	Offset:      0,
})

for _, sandbox := range result.Sandboxes {
	fmt.Printf("ID: %s, State: %s\n", sandbox.ID, sandbox.State)
}
```

##### TerminateSandbox

Terminate a sandbox and get final metrics.

```go
result, err := client.TerminateSandbox(ctx, sandboxID)
fmt.Printf("Terminated: %v\n", result.Terminated)
```

##### GetMetrics

Get server metrics.

```go
metrics, err := client.GetMetrics(ctx, "prometheus")
fmt.Println(metrics)
```

##### Close

Close the client connection.

```go
err := client.Close()
```

### Capabilities

Helper functions for creating capability objects:

```go
import isolate "github.com/josedab/isolate/sdk/go"

caps := []isolate.Capability{
	isolate.Stdout(),              // stdout access
	isolate.Stderr(),              // stderr access
	isolate.Stdin(),               // stdin access
	isolate.FsRead("/data"),       // read from /data
	isolate.FsWrite("/tmp"),       // write to /tmp
	isolate.TempDir(),             // temp directory access
	isolate.HTTP("api.example.com"), // HTTP to specific host
	isolate.DNS(),                 // DNS resolution
	isolate.SystemClock(),         // system clock access
	isolate.MonotonicClock(),      // monotonic clock access
	isolate.Random(),              // secure random access
	isolate.Env("API_KEY"),        // specific env var access
}
```

## TLS Configuration

### Basic TLS

```go
client, err := isolate.NewClient("localhost:50051", isolate.ClientOptions{
	TLS: true,
})
```

### mTLS with Custom Certificates

```go
import (
	"crypto/tls"
	"crypto/x509"
	"os"
)

// Load CA certificate
caCert, _ := os.ReadFile("ca.crt")
caCertPool := x509.NewCertPool()
caCertPool.AppendCertsFromPEM(caCert)

// Load client certificate
clientCert, _ := tls.LoadX509KeyPair("client.crt", "client.key")

client, err := isolate.NewClient("localhost:50051", isolate.ClientOptions{
	TLS:        true,
	RootCAs:    caCertPool,
	ClientCert: &clientCert,
})
```

## Error Handling

All methods return errors that wrap gRPC errors:

```go
result, err := client.CreateSandbox(ctx, wasmBytes, nil)
if err != nil {
	// Check for specific error types
	if status.Code(err) == codes.InvalidArgument {
		log.Printf("Invalid module: %v", err)
	} else if status.Code(err) == codes.ResourceExhausted {
		log.Printf("Resource limit exceeded: %v", err)
	} else {
		log.Printf("Error: %v", err)
	}
}
```

## Requirements

- Go 1.21+
- Running Isolate server

## Proto Regeneration

If you need to regenerate the proto files:

```bash
make proto
```

## License

MIT OR Apache-2.0
