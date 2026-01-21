# @isolate/sdk

TypeScript SDK for the [Isolate](https://github.com/josedab/isolate) gRPC sandbox service. Provides a Promise-based async/await API for creating, running, and managing isolated WebAssembly sandboxes.

## Requirements

- Node.js 18.0 or later
- A running Isolate gRPC server

## Installation

```bash
npm install @isolate/sdk
```

## Quick Start

```typescript
import { IsolateClient, Capabilities } from "@isolate/sdk";
import { readFileSync } from "fs";

async function main() {
  const client = new IsolateClient("localhost:50051");

  try {
    const wasmBytes = readFileSync("module.wasm");

    const result = await client.execute(wasmBytes, {
      memoryLimit: 64 * 1024 * 1024,
      capabilities: [Capabilities.stdout()],
    });

    console.log("Exit code:", result.exitCode);
    console.log("Output:", Buffer.from(result.stdout).toString());
  } finally {
    client.close();
  }
}

main().catch(console.error);
```

## Client Creation

### Basic (Insecure)

```typescript
const client = new IsolateClient("localhost:50051");
```

### With TLS

```typescript
const client = new IsolateClient("isolate.example.com:50051", {
  tls: { secure: true },
});
```

### With mTLS

```typescript
import { readFileSync } from "fs";

const client = new IsolateClient("isolate.example.com:50051", {
  tls: {
    secure: true,
    rootCerts: readFileSync("ca.crt"),
    privateKey: readFileSync("client.key"),
    certChain: readFileSync("client.crt"),
  },
});
```

### Custom Timeout

```typescript
const client = new IsolateClient("localhost:50051", {
  defaultTimeoutMs: 60_000,
});
```

### Factory Function

```typescript
import { createClient } from "@isolate/sdk";

const client = createClient("localhost:50051", {
  defaultTimeoutMs: 10_000,
});
```

## Sandbox Lifecycle

### One-Shot Execution

The `execute` method creates a sandbox, runs it, and terminates it in a single call:

```typescript
const result = await client.execute(wasmBytes, {
  memoryLimit: 64 * 1024 * 1024,
  fuelLimit: 10_000_000,
  wallTimeLimitSecs: 30,
  capabilities: [
    Capabilities.stdout(),
    Capabilities.fsRead("/data"),
  ],
  env: { CONFIG_PATH: "/etc/app/config.json" },
  input: Buffer.from("hello"),
});

console.log("Exit:", result.exitCode);
console.log("Duration:", result.durationMs, "ms");
```

### Reusable Sandbox

For running the same module multiple times:

```typescript
// Create
const { sandboxId, moduleHash } = await client.createSandbox(wasmBytes, {
  memoryLimit: 128 * 1024 * 1024,
  capabilities: [Capabilities.stdout()],
});

// Run multiple times
for (let i = 0; i < 5; i++) {
  const result = await client.runSandbox(
    sandboxId,
    Buffer.from(`iteration ${i}`),
  );
  console.log(`Run ${i}: exit=${result.exitCode}`);
}

// Clean up
const { metrics } = await client.terminateSandbox(sandboxId);
console.log("Total runs:", metrics?.runCount);
```

## API Reference

### `createSandbox(module, config?, timeoutMs?)`

Create a sandbox from a WASM module.

| Parameter | Type | Description |
|-----------|------|-------------|
| `module` | `Uint8Array` | WASM module bytes |
| `config` | `SandboxConfig` | Optional sandbox configuration |
| `timeoutMs` | `number` | Optional per-call timeout |

Returns `Promise<CreateSandboxResponse>`.

### `runSandbox(sandboxId, input?, entryPoint?, timeoutMs?)`

Run an existing sandbox.

| Parameter | Type | Description |
|-----------|------|-------------|
| `sandboxId` | `string` | Sandbox identifier |
| `input` | `Uint8Array \| string` | Optional stdin data |
| `entryPoint` | `string` | Function name (default: `_start`) |
| `timeoutMs` | `number` | Optional per-call timeout |

Returns `Promise<RunSandboxResponse>`.

### `getSandbox(sandboxId, timeoutMs?)`

Get the current status of a sandbox.

Returns `Promise<GetSandboxResponse>`.

### `terminateSandbox(sandboxId, timeoutMs?)`

Terminate a sandbox and retrieve final metrics.

Returns `Promise<TerminateSandboxResponse>`.

### `listSandboxes(options?, timeoutMs?)`

List sandboxes with optional filtering and pagination.

| Parameter | Type | Description |
|-----------|------|-------------|
| `options.stateFilter` | `string` | Filter by state |
| `options.limit` | `number` | Max results |
| `options.offset` | `number` | Pagination offset |

Returns `Promise<ListSandboxesResponse>`.

### `streamOutput(options)`

Stream real-time output from a running sandbox. Returns an `AsyncIterable<OutputChunk>`.

```typescript
const stream = client.streamOutput({
  sandboxId,
  followStdout: true,
  followStderr: true,
});

for await (const chunk of stream) {
  const text = Buffer.from(chunk.data).toString();
  process.stdout.write(`[${chunk.stream}] ${text}`);
}
```

### `getMetrics(format?, timeoutMs?)`

Retrieve server metrics.

| Parameter | Type | Description |
|-----------|------|-------------|
| `format` | `string` | `"prometheus"` or `"json"` |

Returns `Promise<GetMetricsResponse>`.

### `execute(module, config?, timeoutMs?)`

Convenience method: create + run + terminate in one call.

Returns `Promise<RunSandboxResponse & { sandboxId: string }>`.

### `close()`

Close the underlying gRPC channel. The client must not be used after this.

## Capabilities

```typescript
// Standard I/O
Capabilities.stdout()
Capabilities.stderr()
Capabilities.stdin()

// Filesystem
Capabilities.fsRead("/data")
Capabilities.fsWrite("/tmp/output")
Capabilities.tempDir()

// Network
Capabilities.http("api.example.com")
Capabilities.dns()

// Time and Random
Capabilities.systemClock()
Capabilities.monotonicClock()
Capabilities.random()

// Environment
Capabilities.env("API_KEY")
```

## Error Handling

All SDK errors extend `IsolateError`:

```typescript
import {
  IsolateError,
  ConnectionError,
  ValidationError,
  TimeoutError,
  ResourceExhaustedError,
  SandboxNotFoundError,
  PermissionDeniedError,
  GrpcError,
} from "@isolate/sdk";

try {
  await client.runSandbox(sandboxId);
} catch (err) {
  if (err instanceof SandboxNotFoundError) {
    console.error("Sandbox does not exist:", err.sandboxId);
  } else if (err instanceof TimeoutError) {
    console.error("Timed out");
  } else if (err instanceof ResourceExhaustedError) {
    console.error("Resource limit hit:", err.resource);
  } else if (err instanceof ValidationError) {
    console.error("Invalid input:", err.field);
  } else if (err instanceof ConnectionError) {
    console.error("Cannot reach server:", err.address);
  } else if (err instanceof PermissionDeniedError) {
    console.error("Missing capability");
  } else if (err instanceof GrpcError) {
    console.error("gRPC error:", err.grpcCode, err.details);
  } else if (err instanceof IsolateError) {
    console.error("SDK error:", err.code, err.message);
  }
}
```

## Validation Schemas

The SDK exports Zod schemas for runtime validation of all request types:

```typescript
import {
  SandboxConfigSchema,
  CreateSandboxRequestSchema,
} from "@isolate/sdk";

const result = SandboxConfigSchema.safeParse({
  memoryLimit: 64 * 1024 * 1024,
  fuelLimit: -1, // invalid
});

if (!result.success) {
  console.error(result.error.issues);
}
```

## TypeScript Types

All types are exported for use in your own code:

```typescript
import type {
  SandboxConfig,
  RunSandboxResponse,
  ResourceUsage,
  SandboxInfo,
  SandboxMetrics,
  OutputChunk,
  IsolateClientOptions,
  TlsOptions,
  Capability,
} from "@isolate/sdk";
```

## License

MIT
