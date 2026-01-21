/**
 * TypeScript interfaces and Zod validation schemas for all Isolate gRPC
 * request/response types.
 *
 * Each proto message has a corresponding TypeScript interface and a Zod schema
 * that validates values at runtime before they are sent over the wire.
 */

import { z } from "zod";

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

/** A single capability granted to a sandbox. */
export interface Capability {
  /** Capability type (e.g. "stdout", "fs_read", "http"). */
  type: string;
  /** Capability value (path, hostname, etc.). Empty for boolean capabilities. */
  value: string;
}

export const CapabilitySchema = z.object({
  type: z.string().min(1, "Capability type must not be empty"),
  value: z.string(),
});

/**
 * Convenience factory functions for constructing well-known capabilities.
 */
export const Capabilities = {
  stdout: (): Capability => ({ type: "stdout", value: "" }),
  stderr: (): Capability => ({ type: "stderr", value: "" }),
  stdin: (): Capability => ({ type: "stdin", value: "" }),

  fsRead: (path: string): Capability => ({ type: "fs_read", value: path }),
  fsWrite: (path: string): Capability => ({ type: "fs_write", value: path }),
  tempDir: (): Capability => ({ type: "temp_dir", value: "" }),

  http: (host: string): Capability => ({ type: "http", value: host }),
  dns: (): Capability => ({ type: "dns", value: "" }),

  systemClock: (): Capability => ({ type: "system_clock", value: "" }),
  monotonicClock: (): Capability => ({ type: "monotonic_clock", value: "" }),
  random: (): Capability => ({ type: "random", value: "" }),

  env: (name: string): Capability => ({ type: "env", value: name }),
} as const;

// ---------------------------------------------------------------------------
// SandboxConfig
// ---------------------------------------------------------------------------

/** Configuration applied when creating a sandbox. */
export interface SandboxConfig {
  /** Maximum heap memory in bytes. */
  memoryLimit?: number;
  /** Maximum fuel (instruction) budget. */
  fuelLimit?: number;
  /** Maximum wall-clock time in seconds. */
  wallTimeLimitSecs?: number;
  /** Maximum CPU time in seconds. */
  cpuTimeLimitSecs?: number;
  /** Capabilities to grant to the sandbox. */
  capabilities?: Capability[];
  /** Environment variables to set inside the sandbox. */
  env?: Record<string, string>;
  /** Command-line arguments passed to the WASM module. */
  args?: string[];
}

export const SandboxConfigSchema = z.object({
  memoryLimit: z.number().int().nonnegative().optional(),
  fuelLimit: z.number().int().nonnegative().optional(),
  wallTimeLimitSecs: z.number().int().nonnegative().optional(),
  cpuTimeLimitSecs: z.number().int().nonnegative().optional(),
  capabilities: z.array(CapabilitySchema).optional(),
  env: z.record(z.string()).optional(),
  args: z.array(z.string()).optional(),
});

// ---------------------------------------------------------------------------
// ResourceUsage
// ---------------------------------------------------------------------------

/** Resource consumption snapshot returned after a sandbox run. */
export interface ResourceUsage {
  /** Peak memory usage in bytes. */
  peakMemory: number;
  /** Total fuel consumed. */
  fuelConsumed: number;
  /** CPU time in milliseconds. */
  cpuTimeMs: number;
  /** Wall-clock time in milliseconds. */
  wallTimeMs: number;
  /** Total bytes read during execution. */
  bytesRead: number;
  /** Total bytes written during execution. */
  bytesWritten: number;
}

export const ResourceUsageSchema = z.object({
  peakMemory: z.number().nonnegative(),
  fuelConsumed: z.number().nonnegative(),
  cpuTimeMs: z.number().nonnegative(),
  wallTimeMs: z.number().nonnegative(),
  bytesRead: z.number().nonnegative(),
  bytesWritten: z.number().nonnegative(),
});

// ---------------------------------------------------------------------------
// SandboxMetrics
// ---------------------------------------------------------------------------

/** Cumulative metrics for a sandbox across all runs. */
export interface SandboxMetrics {
  /** Total number of runs. */
  runCount: number;
  /** Number of successful (exit code 0) runs. */
  successCount: number;
  /** Number of failed runs. */
  failureCount: number;
  /** Sum of all run durations in milliseconds. */
  totalRunDurationMs: number;
  /** Duration of the most recent run in milliseconds. */
  lastRunDurationMs: number;
}

export const SandboxMetricsSchema = z.object({
  runCount: z.number().nonnegative(),
  successCount: z.number().nonnegative(),
  failureCount: z.number().nonnegative(),
  totalRunDurationMs: z.number().nonnegative(),
  lastRunDurationMs: z.number().nonnegative(),
});

// ---------------------------------------------------------------------------
// SandboxInfo
// ---------------------------------------------------------------------------

/** Full status information about an existing sandbox. */
export interface SandboxInfo {
  /** Unique sandbox identifier. */
  id: string;
  /** Current lifecycle state (e.g. "ready", "running", "terminated"). */
  state: string;
  /** SHA-256 hash of the WASM module used to create this sandbox. */
  moduleHash: string;
  /** Unix timestamp (milliseconds) when the sandbox was created. */
  createdAt: number;
  /** Seconds since creation. */
  ageSecs: number;
  /** Cumulative run metrics. */
  metrics?: SandboxMetrics;
}

export const SandboxInfoSchema = z.object({
  id: z.string(),
  state: z.string(),
  moduleHash: z.string(),
  createdAt: z.number(),
  ageSecs: z.number().nonnegative(),
  metrics: SandboxMetricsSchema.optional(),
});

// ---------------------------------------------------------------------------
// CreateSandbox
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.createSandbox}. */
export interface CreateSandboxRequest {
  /** WASM module bytes. */
  module: Uint8Array;
  /** Sandbox configuration. */
  config?: SandboxConfig;
}

export const CreateSandboxRequestSchema = z.object({
  module: z.instanceof(Uint8Array).refine((v) => v.byteLength > 0, {
    message: "WASM module must not be empty",
  }),
  config: SandboxConfigSchema.optional(),
});

/** Result returned by {@link IsolateClient.createSandbox}. */
export interface CreateSandboxResponse {
  /** Unique sandbox identifier. */
  sandboxId: string;
  /** SHA-256 hash of the WASM module. */
  moduleHash: string;
  /** Time in milliseconds taken to create the sandbox. */
  creationTimeMs: number;
}

// ---------------------------------------------------------------------------
// RunSandbox
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.runSandbox}. */
export interface RunSandboxRequest {
  /** Sandbox identifier returned by createSandbox. */
  sandboxId: string;
  /** Optional input data provided to the sandbox via stdin. */
  input?: Uint8Array;
  /** Entry-point function name (defaults to "_start"). */
  entryPoint?: string;
}

export const RunSandboxRequestSchema = z.object({
  sandboxId: z.string().min(1, "sandboxId must not be empty"),
  input: z.instanceof(Uint8Array).optional(),
  entryPoint: z.string().optional(),
});

/** Result returned by {@link IsolateClient.runSandbox}. */
export interface RunSandboxResponse {
  /** Process exit code. */
  exitCode: number;
  /** Captured standard output. */
  stdout: Uint8Array;
  /** Captured standard error. */
  stderr: Uint8Array;
  /** Execution duration in milliseconds. */
  durationMs: number;
  /** Resource consumption during this run. */
  resourceUsage?: ResourceUsage;
}

// ---------------------------------------------------------------------------
// GetSandbox
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.getSandbox}. */
export interface GetSandboxRequest {
  /** Sandbox identifier. */
  sandboxId: string;
}

export const GetSandboxRequestSchema = z.object({
  sandboxId: z.string().min(1, "sandboxId must not be empty"),
});

/** Result returned by {@link IsolateClient.getSandbox}. */
export interface GetSandboxResponse {
  /** Full sandbox information. */
  sandbox: SandboxInfo;
}

// ---------------------------------------------------------------------------
// TerminateSandbox
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.terminateSandbox}. */
export interface TerminateSandboxRequest {
  /** Sandbox identifier. */
  sandboxId: string;
}

export const TerminateSandboxRequestSchema = z.object({
  sandboxId: z.string().min(1, "sandboxId must not be empty"),
});

/** Result returned by {@link IsolateClient.terminateSandbox}. */
export interface TerminateSandboxResponse {
  /** Whether the sandbox was actually terminated (false if already stopped). */
  terminated: boolean;
  /** Final cumulative metrics. */
  metrics?: SandboxMetrics;
}

// ---------------------------------------------------------------------------
// ListSandboxes
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.listSandboxes}. */
export interface ListSandboxesRequest {
  /** Optional state filter (e.g. "ready", "running"). */
  stateFilter?: string;
  /** Maximum number of sandboxes to return. */
  limit?: number;
  /** Pagination offset. */
  offset?: number;
}

export const ListSandboxesRequestSchema = z.object({
  stateFilter: z.string().optional(),
  limit: z.number().int().nonnegative().optional(),
  offset: z.number().int().nonnegative().optional(),
});

/** Result returned by {@link IsolateClient.listSandboxes}. */
export interface ListSandboxesResponse {
  /** List of sandbox info objects. */
  sandboxes: SandboxInfo[];
  /** Total number of sandboxes matching the filter (ignoring pagination). */
  total: number;
}

// ---------------------------------------------------------------------------
// StreamOutput
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.streamOutput}. */
export interface StreamOutputRequest {
  /** Sandbox identifier. */
  sandboxId: string;
  /** Whether to follow stdout. */
  followStdout?: boolean;
  /** Whether to follow stderr. */
  followStderr?: boolean;
}

export const StreamOutputRequestSchema = z.object({
  sandboxId: z.string().min(1, "sandboxId must not be empty"),
  followStdout: z.boolean().optional(),
  followStderr: z.boolean().optional(),
});

/** A single chunk of sandbox output received via the streaming RPC. */
export interface OutputChunk {
  /** Stream name: "stdout" or "stderr". */
  stream: string;
  /** Raw data bytes. */
  data: Uint8Array;
  /** Unix timestamp (milliseconds) when this chunk was produced. */
  timestamp: number;
}

// ---------------------------------------------------------------------------
// GetMetrics
// ---------------------------------------------------------------------------

/** Options for {@link IsolateClient.getMetrics}. */
export interface GetMetricsRequest {
  /** Desired format: "prometheus" or "json". */
  format?: string;
}

export const GetMetricsRequestSchema = z.object({
  format: z.string().optional(),
});

/** Result returned by {@link IsolateClient.getMetrics}. */
export interface GetMetricsResponse {
  /** Serialized metrics payload. */
  data: string;
}

// ---------------------------------------------------------------------------
// Client Options
// ---------------------------------------------------------------------------

/** TLS configuration for the client connection. */
export interface TlsOptions {
  /** Enable TLS (defaults to false for plaintext). */
  secure: boolean;
  /** PEM-encoded root CA certificate(s) for server verification. */
  rootCerts?: Buffer;
  /** PEM-encoded client private key for mTLS. */
  privateKey?: Buffer;
  /** PEM-encoded client certificate chain for mTLS. */
  certChain?: Buffer;
}

/** Configuration options for the Isolate client. */
export interface IsolateClientOptions {
  /** TLS configuration. When omitted the connection is plaintext. */
  tls?: TlsOptions;
  /**
   * Default timeout in milliseconds applied to every unary RPC. Individual
   * calls can override this via their own deadline. Defaults to 30 000 ms.
   */
  defaultTimeoutMs?: number;
  /**
   * Additional default gRPC channel options forwarded to @grpc/grpc-js.
   * See grpc.ChannelOptions for the full set of supported keys.
   */
  channelOptions?: Record<string, string | number>;
}
