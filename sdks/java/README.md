# Isolate Java SDK

Java client library for the [Isolate](https://github.com/isolate-project/isolate) gRPC sandbox service. Provides a type-safe, builder-based API for creating, running, and managing WebAssembly sandboxes with strong isolation guarantees.

## Requirements

- Java 11 or later
- Gradle 7.0 or later (for building)

## Installation

### Gradle

```groovy
dependencies {
    implementation 'io.isolate:isolate-sdk:0.1.0'
}
```

### Maven

```xml
<dependency>
    <groupId>io.isolate</groupId>
    <artifactId>isolate-sdk</artifactId>
    <version>0.1.0</version>
</dependency>
```

## Quick Start

```java
import io.isolate.sdk.IsolateClient;
import io.isolate.sdk.IsolateClientConfig;
import io.isolate.sdk.models.SandboxConfig;
import io.isolate.sdk.models.RunResult;

// Configure the client
IsolateClientConfig config = IsolateClientConfig.builder()
    .target("localhost:50051")
    .build();

// Use try-with-resources for automatic cleanup
try (IsolateClient client = IsolateClient.create(config)) {
    // Read your WASM module
    byte[] wasmModule = Files.readAllBytes(Path.of("my_module.wasm"));

    // Configure the sandbox
    SandboxConfig sandboxConfig = SandboxConfig.builder()
        .memoryLimit(64 * 1024 * 1024)  // 64 MB
        .fuelLimit(1_000_000)
        .wallTimeLimitSecs(30)
        .addCapability("stdout")
        .addCapability("fs_read", "/data")
        .putEnv("MODE", "production")
        .build();

    // Create and run the sandbox
    String sandboxId = client.createSandbox(wasmModule, sandboxConfig);
    RunResult result = client.runSandbox(sandboxId, new byte[0]);

    System.out.println("Exit code: " + result.getExitCode());
    System.out.println("Output: " + result.getStdoutString());

    // Check resource usage
    result.getResourceUsage().ifPresent(usage -> {
        System.out.println("Peak memory: " + usage.getPeakMemory() + " bytes");
        System.out.println("Fuel consumed: " + usage.getFuelConsumed());
        System.out.println("Wall time: " + usage.getWallTimeMs() + " ms");
    });
}
```

## Async Operations

All methods have async variants that return `CompletableFuture`:

```java
client.createSandboxAsync(wasmModule, sandboxConfig)
    .thenCompose(sandboxId -> client.runSandboxAsync(sandboxId, input))
    .thenAccept(result -> {
        System.out.println("Exit code: " + result.getExitCode());
        System.out.println("Output: " + result.getStdoutString());
    })
    .exceptionally(ex -> {
        System.err.println("Failed: " + ex.getMessage());
        return null;
    });
```

## Client Configuration

```java
IsolateClientConfig config = IsolateClientConfig.builder()
    .target("localhost:50051")              // Required: gRPC server address
    .timeout(Duration.ofSeconds(30))        // Per-call timeout (default: 30s)
    .maxRetries(3)                          // Retry count for transient failures (default: 3)
    .retryBackoff(Duration.ofMillis(100))   // Initial retry backoff (default: 100ms)
    .maxMessageSize(16 * 1024 * 1024)       // Max message size (default: 16 MB)
    .keepAliveEnabled(true)                 // Enable gRPC keep-alive
    .keepAliveTime(Duration.ofSeconds(30))  // Keep-alive ping interval
    .keepAliveTimeout(Duration.ofSeconds(10)) // Keep-alive ping timeout
    .build();
```

### TLS Configuration

```java
IsolateClientConfig config = IsolateClientConfig.builder()
    .target("isolate.example.com:443")
    .tlsEnabled(true)
    .tlsCaCertPath("/path/to/ca.pem")       // CA certificate for server verification
    .tlsCertPath("/path/to/client.pem")     // Client certificate (for mTLS)
    .tlsKeyPath("/path/to/client-key.pem")  // Client private key (for mTLS)
    .build();
```

## API Reference

### Sandbox Lifecycle

| Method | Async Variant | Description |
|--------|---------------|-------------|
| `createSandbox(byte[], SandboxConfig)` | `createSandboxAsync(...)` | Create a new sandbox |
| `runSandbox(String, byte[])` | `runSandboxAsync(...)` | Run a sandbox |
| `runSandbox(String, byte[], String)` | `runSandboxAsync(...)` | Run with custom entry point |
| `getSandbox(String)` | `getSandboxAsync(...)` | Get sandbox info |
| `terminateSandbox(String)` | `terminateSandboxAsync(...)` | Terminate a sandbox |
| `listSandboxes()` | `listSandboxesAsync()` | List all sandboxes |
| `listSandboxes(String, int, int)` | `listSandboxesAsync(...)` | List with filter/pagination |
| `getMetrics(String)` | `getMetricsAsync(...)` | Get service metrics |

### Sandbox Configuration

```java
SandboxConfig config = SandboxConfig.builder()
    .memoryLimit(64 * 1024 * 1024)    // Memory limit in bytes
    .fuelLimit(1_000_000)             // Execution fuel limit
    .wallTimeLimitSecs(30)            // Wall-clock timeout
    .cpuTimeLimitSecs(10)             // CPU time limit
    .addCapability("stdout")          // Grant stdout access
    .addCapability("stderr")          // Grant stderr access
    .addCapability("fs_read", "/data") // Grant filesystem read
    .putEnv("KEY", "VALUE")           // Set environment variable
    .addArg("--flag")                 // Add CLI argument
    .build();
```

## Exception Handling

All SDK exceptions extend `IsolateException`:

```java
try {
    client.runSandbox(sandboxId, input);
} catch (SandboxNotFoundException e) {
    // Sandbox does not exist
    System.err.println("Not found: " + e.getSandboxId());
} catch (SandboxExecutionException e) {
    // Execution failure (resource exhaustion, permission denied, etc.)
    System.err.println("Execution failed: " + e.getMessage());
    e.getExitCode().ifPresent(code -> System.err.println("Exit code: " + code));
} catch (IsolateException e) {
    // Other SDK errors (connection, timeout, etc.)
    System.err.println("Error: " + e.getMessage());
    System.err.println("Operation: " + e.getOperation());
    System.err.println("gRPC code: " + e.getGrpcStatusCode());
}
```

### Exception Hierarchy

- `IsolateException` - Base exception for all SDK errors
  - `SandboxNotFoundException` - Sandbox does not exist (maps to gRPC `NOT_FOUND`)
  - `SandboxExecutionException` - Execution failure (resource exhaustion, permission denied, etc.)

## Retry Behavior

The client automatically retries operations that fail with transient gRPC status codes:

- `UNAVAILABLE` - Server is temporarily unreachable
- `DEADLINE_EXCEEDED` - Request timed out
- `ABORTED` - Operation was aborted

Retries use exponential backoff starting from the configured `retryBackoff` duration, capped at 10 seconds. Non-transient errors (such as `NOT_FOUND` or `INVALID_ARGUMENT`) are not retried.

## Building from Source

```bash
# Build the SDK
./gradlew build

# Run tests
./gradlew test

# Generate Javadoc
./gradlew javadoc

# Publish to local Maven repository
./gradlew publishToMavenLocal
```

## License

MIT License. See the project root for the full license text.
