# @isolate/client

TypeScript/JavaScript client SDK for [Isolate](https://github.com/josedab/isolate), a secure sandbox runtime for WebAssembly.

## Installation

```bash
npm install @isolate/client
# or
yarn add @isolate/client
# or
pnpm add @isolate/client
```

## Quick Start

```typescript
import { IsolateClient, Capabilities } from '@isolate/client';
import { readFileSync } from 'fs';

// Create a client
const client = new IsolateClient('localhost:50051');

// Load your WASM module
const wasmBytes = readFileSync('module.wasm');

// Create and run a sandbox
const result = await client.execute(wasmBytes, {
  memoryLimit: 64 * 1024 * 1024, // 64MB
  fuelLimit: 10_000_000,         // 10M instructions
  capabilities: [
    Capabilities.stdout(),
    Capabilities.stderr(),
  ],
});

console.log('Exit code:', result.exitCode);
console.log('Stdout:', result.stdout.toString());

// Don't forget to close the connection
client.close();
```

## API Reference

### IsolateClient

The main client class for interacting with the Isolate server.

#### Constructor

```typescript
new IsolateClient(address: string, options?: IsolateClientOptions)
```

- `address` - Server address (e.g., `'localhost:50051'`)
- `options` - Optional configuration
  - `secure` - Use TLS connection
  - `rootCerts` - Root certificates for TLS
  - `privateKey` - Client private key for mTLS
  - `certChain` - Client certificate chain for mTLS

#### Methods

##### `createSandbox(module, options)`

Create a new sandbox with the given WASM module.

```typescript
const { sandboxId, moduleHash } = await client.createSandbox(wasmBytes, {
  memoryLimit: 64 * 1024 * 1024,
  fuelLimit: 10_000_000,
  wallTimeLimitSecs: 30,
  capabilities: [Capabilities.stdout()],
  env: { API_KEY: 'secret' },
  args: ['--verbose'],
});
```

##### `runSandbox(sandboxId, options)`

Run an existing sandbox.

```typescript
const result = await client.runSandbox(sandboxId, {
  input: Buffer.from('Hello'),
  entryPoint: '_start',
});

console.log(result.exitCode);
console.log(result.stdout.toString());
console.log(result.resourceUsage.peakMemory);
```

##### `execute(module, options)`

Convenience method that creates, runs, and terminates a sandbox in one call.

```typescript
const result = await client.execute(wasmBytes, {
  capabilities: [Capabilities.stdout()],
});
```

##### `getSandbox(sandboxId)`

Get sandbox status and metrics.

```typescript
const info = await client.getSandbox(sandboxId);
console.log(info.state, info.metrics);
```

##### `listSandboxes(options)`

List all sandboxes.

```typescript
const { sandboxes, total } = await client.listSandboxes({
  stateFilter: 'ready',
  limit: 10,
  offset: 0,
});
```

##### `terminateSandbox(sandboxId)`

Terminate a sandbox and get final metrics.

```typescript
const { terminated, metrics } = await client.terminateSandbox(sandboxId);
```

##### `getMetrics(format)`

Get server metrics.

```typescript
const metrics = await client.getMetrics('prometheus');
```

##### `close()`

Close the client connection.

```typescript
client.close();
```

### Capabilities

Helper functions for creating capability objects:

```typescript
import { Capabilities } from '@isolate/client';

const caps = [
  Capabilities.stdout(),           // stdout access
  Capabilities.stderr(),           // stderr access
  Capabilities.stdin(),            // stdin access
  Capabilities.fsRead('/data'),    // read from /data
  Capabilities.fsWrite('/tmp'),    // write to /tmp
  Capabilities.tempDir(),          // temp directory access
  Capabilities.http('api.example.com'), // HTTP to specific host
  Capabilities.dns(),              // DNS resolution
  Capabilities.systemClock(),      // system clock access
  Capabilities.monotonicClock(),   // monotonic clock access
  Capabilities.random(),           // secure random access
  Capabilities.env('API_KEY'),     // specific env var access
];
```

## TypeScript Support

This package includes full TypeScript type definitions. All types are exported:

```typescript
import type {
  Capability,
  CreateSandboxOptions,
  RunSandboxOptions,
  RunSandboxResult,
  SandboxInfo,
  SandboxMetrics,
  ResourceUsage,
} from '@isolate/client';
```

## Requirements

- Node.js >= 18.0.0
- Running Isolate server

## License

MIT OR Apache-2.0
