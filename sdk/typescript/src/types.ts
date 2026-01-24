/**
 * Type definitions for the Isolate TypeScript SDK.
 */

/**
 * Client connection options.
 */
export interface IsolateClientOptions {
  /**
   * Path to the proto file. Defaults to bundled proto.
   */
  protoPath?: string;

  /**
   * Use secure (TLS) connection.
   */
  secure?: boolean;

  /**
   * Root certificates for TLS.
   */
  rootCerts?: Buffer;

  /**
   * Client private key for mTLS.
   */
  privateKey?: Buffer;

  /**
   * Client certificate chain for mTLS.
   */
  certChain?: Buffer;

  /**
   * Connection timeout in milliseconds.
   */
  timeout?: number;
}

/**
 * Capability types supported by Isolate.
 */
export type CapabilityType =
  | 'stdout'
  | 'stderr'
  | 'stdin'
  | 'fs:read'
  | 'fs:write'
  | 'fs:temp'
  | 'http'
  | 'dns'
  | 'time:system'
  | 'time:monotonic'
  | 'random'
  | 'env';

/**
 * A capability to grant to a sandbox.
 */
export interface Capability {
  /**
   * Type of capability.
   */
  type: CapabilityType;

  /**
   * Value for the capability (path for fs, host for http, var name for env).
   */
  value?: string;
}

/**
 * Options for creating a sandbox.
 */
export interface CreateSandboxOptions {
  /**
   * Memory limit in bytes.
   */
  memoryLimit?: number;

  /**
   * CPU fuel limit (instruction count).
   */
  fuelLimit?: number;

  /**
   * Wall-clock time limit in seconds.
   */
  wallTimeLimitSecs?: number;

  /**
   * CPU time limit in seconds.
   */
  cpuTimeLimitSecs?: number;

  /**
   * Capabilities to grant.
   */
  capabilities?: Capability[];

  /**
   * Environment variables to pass.
   */
  env?: Record<string, string>;

  /**
   * Command-line arguments to pass.
   */
  args?: string[];
}

/**
 * Options for running a sandbox.
 */
export interface RunSandboxOptions {
  /**
   * Input data to provide to stdin.
   */
  input?: Buffer | Uint8Array | string;

  /**
   * Entry point function name.
   * @default '_start'
   */
  entryPoint?: string;
}

/**
 * Resource usage information.
 */
export interface ResourceUsage {
  /**
   * Peak memory usage in bytes.
   */
  peakMemory: number;

  /**
   * CPU fuel consumed.
   */
  fuelConsumed: number;

  /**
   * CPU time in milliseconds.
   */
  cpuTimeMs: number;

  /**
   * Wall-clock time in milliseconds.
   */
  wallTimeMs: number;

  /**
   * Bytes read.
   */
  bytesRead: number;

  /**
   * Bytes written.
   */
  bytesWritten: number;
}

/**
 * Result from running a sandbox.
 */
export interface RunSandboxResult {
  /**
   * Exit code from the WASM module.
   */
  exitCode: number;

  /**
   * Captured stdout.
   */
  stdout: Buffer;

  /**
   * Captured stderr.
   */
  stderr: Buffer;

  /**
   * Execution duration in milliseconds.
   */
  durationMs: number;

  /**
   * Resource usage during execution.
   */
  resourceUsage?: ResourceUsage;
}

/**
 * Sandbox execution metrics.
 */
export interface SandboxMetrics {
  /**
   * Total number of runs.
   */
  runCount: number;

  /**
   * Number of successful runs.
   */
  successCount: number;

  /**
   * Number of failed runs.
   */
  failureCount: number;

  /**
   * Total run duration in milliseconds.
   */
  totalRunDurationMs: number;

  /**
   * Last run duration in milliseconds.
   */
  lastRunDurationMs: number;
}

/**
 * Information about a sandbox.
 */
export interface SandboxInfo {
  /**
   * Unique sandbox identifier.
   */
  id: string;

  /**
   * Current sandbox state.
   */
  state: string;

  /**
   * Hash of the WASM module.
   */
  moduleHash: string;

  /**
   * Creation timestamp.
   */
  createdAt: Date;

  /**
   * Age in seconds.
   */
  ageSecs: number;

  /**
   * Sandbox metrics.
   */
  metrics?: SandboxMetrics;
}

/**
 * Options for listing sandboxes.
 */
export interface ListSandboxesOptions {
  /**
   * Filter by state.
   */
  stateFilter?: string;

  /**
   * Maximum number of results.
   */
  limit?: number;

  /**
   * Pagination offset.
   */
  offset?: number;
}

/**
 * Capability helper functions.
 */
export const Capabilities = {
  /**
   * Grant stdout access.
   */
  stdout: (): Capability => ({ type: 'stdout' }),

  /**
   * Grant stderr access.
   */
  stderr: (): Capability => ({ type: 'stderr' }),

  /**
   * Grant stdin access.
   */
  stdin: (): Capability => ({ type: 'stdin' }),

  /**
   * Grant filesystem read access.
   */
  fsRead: (path: string): Capability => ({ type: 'fs:read', value: path }),

  /**
   * Grant filesystem write access.
   */
  fsWrite: (path: string): Capability => ({ type: 'fs:write', value: path }),

  /**
   * Grant temp directory access.
   */
  tempDir: (): Capability => ({ type: 'fs:temp' }),

  /**
   * Grant HTTP client access.
   */
  http: (hostPattern: string): Capability => ({ type: 'http', value: hostPattern }),

  /**
   * Grant DNS resolution access.
   */
  dns: (): Capability => ({ type: 'dns' }),

  /**
   * Grant system clock access.
   */
  systemClock: (): Capability => ({ type: 'time:system' }),

  /**
   * Grant monotonic clock access.
   */
  monotonicClock: (): Capability => ({ type: 'time:monotonic' }),

  /**
   * Grant secure random access.
   */
  random: (): Capability => ({ type: 'random' }),

  /**
   * Grant environment variable access.
   */
  env: (varName: string): Capability => ({ type: 'env', value: varName }),
} as const;
