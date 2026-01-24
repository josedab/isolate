---
sidebar_position: 12
---

# TypeScript SDK

The TypeScript SDK provides a client library for interacting with the Isolate gRPC server from Node.js applications.

## Installation

```bash
npm install @isolate/client
```

**Requirements:**
- Node.js 18.0 or later
- Running Isolate gRPC server

## Quick Start

```typescript
import { IsolateClient, Capabilities } from '@isolate/client';
import { readFileSync } from 'fs';

async function main() {
  // Connect to the Isolate server
  const client = new IsolateClient('localhost:50051');

  // Load a WASM module
  const wasmBytes = readFileSync('module.wasm');

  // Execute the module
  const result = await client.execute(wasmBytes, {
    memoryLimit: 64 * 1024 * 1024, // 64MB
    capabilities: [Capabilities.stdout()],
  });

  console.log('Exit code:', result.exitCode);
  console.log('Output:', result.stdout.toString());

  client.close();
}

main().catch(console.error);
```

## Client Creation

### Basic Connection

```typescript
import { IsolateClient } from '@isolate/client';

const client = new IsolateClient('localhost:50051');
// ... use client
client.close();
```

### With TLS

```typescript
const client = new IsolateClient('isolate.example.com:50051', {
  secure: true,
});
```

### With mTLS (Mutual TLS)

```typescript
import { readFileSync } from 'fs';

const client = new IsolateClient('isolate.example.com:50051', {
  secure: true,
  rootCerts: readFileSync('ca.crt'),
  privateKey: readFileSync('client.key'),
  certChain: readFileSync('client.crt'),
});
```

### Factory Function

You can also use the factory function:

```typescript
import { createClient } from '@isolate/client';

const client = createClient('localhost:50051', {
  timeout: 30000, // Connection timeout in ms
});
```

## Sandbox Lifecycle

### One-Shot Execution

The simplest way to run a WASM module. Creates a sandbox, runs it, and terminates it:

```typescript
const result = await client.execute(wasmBytes, {
  memoryLimit: 64 * 1024 * 1024,
  fuelLimit: 10_000_000,
  wallTimeLimit: 30, // seconds
  capabilities: [
    Capabilities.stdout(),
    Capabilities.fsRead('/data'),
  ],
  env: {
    CONFIG_PATH: '/etc/app/config.json',
  },
});

console.log('Exit:', result.exitCode);
console.log('Duration:', result.durationMs, 'ms');
```

### Reusable Sandbox

For running the same module multiple times, create a sandbox once and reuse it:

```typescript
// Create the sandbox
const { sandboxId, moduleHash } = await client.createSandbox(wasmBytes, {
  memoryLimit: 128 * 1024 * 1024,
  capabilities: [Capabilities.stdout()],
});

console.log('Created sandbox:', sandboxId);
console.log('Module hash:', moduleHash);

// Run multiple times
for (let i = 0; i < 5; i++) {
  const result = await client.runSandbox(sandboxId, {
    input: Buffer.from(`iteration ${i}`),
  });
  console.log(`Run ${i}: exit=${result.exitCode}, duration=${result.durationMs}ms`);
}

// Clean up
const { metrics } = await client.terminateSandbox(sandboxId);
console.log('Total runs:', metrics?.runCount);
```

## Capabilities

Capabilities control what the WASM module can access. By default, modules have no capabilities.

### Standard I/O

```typescript
Capabilities.stdout()   // Write to stdout
Capabilities.stderr()   // Write to stderr
Capabilities.stdin()    // Read from stdin
```

### Filesystem

```typescript
Capabilities.fsRead('/data')        // Read files under /data
Capabilities.fsWrite('/tmp/output') // Write files under /tmp/output
Capabilities.tempDir()              // Access to temporary directory
```

### Network

```typescript
Capabilities.http('api.example.com')    // HTTP access to specific host
Capabilities.http('*.example.com')      // HTTP access with wildcard
Capabilities.dns()                      // DNS resolution
```

### Time and Random

```typescript
Capabilities.systemClock()    // Wall clock time
Capabilities.monotonicClock() // Monotonic clock for durations
Capabilities.random()         // Cryptographic random numbers
```

### Environment Variables

```typescript
Capabilities.env('API_KEY')     // Access to specific env var
Capabilities.env('CONFIG_PATH') // Access to another env var
```

### Example: Combining Capabilities

```typescript
const result = await client.execute(wasmBytes, {
  memoryLimit: 64 * 1024 * 1024,
  capabilities: [
    Capabilities.stdout(),
    Capabilities.stderr(),
    Capabilities.fsRead('/data/input'),
    Capabilities.fsWrite('/data/output'),
    Capabilities.http('api.example.com'),
    Capabilities.systemClock(),
    Capabilities.env('API_KEY'),
  ],
  env: {
    API_KEY: 'secret-token',
  },
});
```

## Resource Limits

Control resource consumption to prevent abuse:

```typescript
const result = await client.execute(wasmBytes, {
  memoryLimit: 128 * 1024 * 1024, // 128MB heap
  fuelLimit: 50_000_000,          // ~50M instructions
  wallTimeLimit: 60,              // 60 seconds max
  cpuTimeLimit: 30,               // 30 seconds CPU time
});
```

| Option | Description | Default |
|--------|-------------|---------|
| `memoryLimit` | Maximum heap memory in bytes | Server default |
| `fuelLimit` | Maximum instructions (fuel units) | Unlimited |
| `wallTimeLimit` | Maximum wall-clock time in seconds | Server default |
| `cpuTimeLimit` | Maximum CPU time in seconds | Server default |

## Input Types

The SDK accepts multiple input types for WASM modules and stdin:

```typescript
// Buffer
await client.execute(Buffer.from(wasmBytes), options);

// Uint8Array
await client.execute(new Uint8Array(wasmBytes), options);

// For stdin input
await client.runSandbox(sandboxId, {
  input: Buffer.from('hello'),           // Buffer
  input: new Uint8Array([72, 101, 108]), // Uint8Array
  input: 'hello world',                   // String (UTF-8 encoded)
});
```

## Inspecting Sandboxes

### Get Sandbox Info

```typescript
const info = await client.getSandbox(sandboxId);

console.log('ID:', info.id);
console.log('State:', info.state);
console.log('Module Hash:', info.moduleHash);
console.log('Created:', info.createdAt);  // Date object
console.log('Age:', info.ageSecs, 'seconds');
console.log('Run Count:', info.metrics?.runCount);
```

### List Sandboxes

```typescript
// List all sandboxes
const { sandboxes, total } = await client.listSandboxes();

// List with filters
const result = await client.listSandboxes({
  stateFilter: 'ready',  // Filter by state
  limit: 10,             // Pagination
  offset: 0,
});

for (const sandbox of result.sandboxes) {
  console.log(`${sandbox.id}: ${sandbox.state} (${sandbox.metrics?.runCount} runs)`);
}
```

## Metrics

Retrieve server metrics in Prometheus or JSON format:

```typescript
// Prometheus format (default)
const prometheusMetrics = await client.getMetrics('prometheus');
console.log(prometheusMetrics);

// JSON format
const jsonMetrics = await client.getMetrics('json');
console.log(jsonMetrics);
```

## Error Handling

All methods return Promises that reject with Error objects containing context:

```typescript
try {
  const result = await client.execute(wasmBytes, options);
} catch (err) {
  console.error('Error:', err.message);
  console.error('Cause:', err.cause);  // Original gRPC error
}
```

### Typed Error Handling

```typescript
async function runWithRetry(
  client: IsolateClient,
  wasmBytes: Buffer,
  retries = 3
) {
  for (let i = 0; i < retries; i++) {
    try {
      return await client.execute(wasmBytes, {
        capabilities: [Capabilities.stdout()],
      });
    } catch (err) {
      const message = err.message.toLowerCase();

      if (message.includes('resource exhausted')) {
        console.log('Resource limit hit, retrying with higher limits...');
        continue;
      }

      if (message.includes('deadline exceeded')) {
        console.log('Timeout, retrying...');
        continue;
      }

      if (message.includes('invalid')) {
        throw err; // Don't retry invalid input
      }

      if (i === retries - 1) throw err;
    }
  }
}
```

### Common Errors

| Error Contains | Meaning |
|----------------|---------|
| `invalid` | Invalid WASM module or configuration |
| `resource exhausted` | Memory or fuel limit exceeded |
| `deadline exceeded` | Execution timeout |
| `not found` | Sandbox ID doesn't exist |
| `permission denied` | Capability not granted |
| `unavailable` | Server connection issue |

## TypeScript Types

The SDK exports all types for TypeScript users:

```typescript
import type {
  IsolateClientOptions,
  CreateSandboxOptions,
  RunSandboxOptions,
  RunSandboxResult,
  ResourceUsage,
  SandboxInfo,
  SandboxMetrics,
  Capability,
  CapabilityType,
} from '@isolate/client';
```

### Type Examples

```typescript
// Strongly typed options
const options: CreateSandboxOptions = {
  memoryLimit: 64 * 1024 * 1024,
  fuelLimit: 10_000_000,
  capabilities: [Capabilities.stdout()],
  env: { KEY: 'value' },
  args: ['--flag'],
};

// Strongly typed results
const result: RunSandboxResult = await client.execute(wasmBytes, options);

// Access typed resource usage
const usage: ResourceUsage = result.resourceUsage;
console.log('Peak memory:', usage.peakMemory);
console.log('Fuel consumed:', usage.fuelConsumed);
```

## Complete Example

```typescript
import { IsolateClient, Capabilities } from '@isolate/client';
import { readFileSync } from 'fs';

async function processData(inputPath: string): Promise<void> {
  const client = new IsolateClient('localhost:50051');

  try {
    // Load the processor module
    const wasmBytes = readFileSync('processor.wasm');
    const inputData = readFileSync(inputPath);

    // Execute with full configuration
    const result = await client.execute(wasmBytes, {
      memoryLimit: 64 * 1024 * 1024,
      fuelLimit: 10_000_000,
      wallTimeLimit: 30,
      capabilities: [
        Capabilities.stdout(),
        Capabilities.stderr(),
        Capabilities.fsRead('/data'),
        Capabilities.systemClock(),
      ],
      args: ['--verbose', '--format=json'],
      env: {
        LOG_LEVEL: 'debug',
      },
      input: inputData,
    });

    // Process results
    console.log('Exit Code:', result.exitCode);
    console.log('Duration:', result.durationMs, 'ms');
    console.log('Memory Peak:', result.resourceUsage.peakMemory, 'bytes');
    console.log('Fuel Used:', result.resourceUsage.fuelConsumed);

    if (result.exitCode === 0) {
      console.log('Output:');
      console.log(result.stdout.toString());
    } else {
      console.error('Error:');
      console.error(result.stderr.toString());
    }
  } finally {
    client.close();
  }
}

// Run with async/await
processData('input.json').catch(console.error);
```

## ESM and CommonJS

The SDK supports both module systems:

```typescript
// ESM (import)
import { IsolateClient, Capabilities } from '@isolate/client';

// CommonJS (require)
const { IsolateClient, Capabilities } = require('@isolate/client');
```

## See Also

- [gRPC Server](./grpc-server) - Running the Isolate server
- [Go SDK](./sdk-go) - Go client
- [API Reference](../reference/api) - Core API documentation
