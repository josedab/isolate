/**
 * @isolate/client - TypeScript/JavaScript SDK for Isolate
 *
 * A client library for interacting with the Isolate secure sandbox runtime.
 *
 * @example
 * ```typescript
 * import { IsolateClient } from '@isolate/client';
 * import { readFileSync } from 'fs';
 *
 * const client = new IsolateClient('localhost:50051');
 *
 * // Create a sandbox
 * const wasmBytes = readFileSync('module.wasm');
 * const { sandboxId } = await client.createSandbox(wasmBytes, {
 *   memoryLimit: 64 * 1024 * 1024,
 *   capabilities: [{ type: 'stdout' }],
 * });
 *
 * // Run the sandbox
 * const result = await client.runSandbox(sandboxId);
 * console.log('Exit code:', result.exitCode);
 * console.log('Stdout:', result.stdout.toString());
 *
 * // Cleanup
 * await client.terminateSandbox(sandboxId);
 * ```
 */

import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import * as path from 'path';

// Re-export types
export * from './types';

import type {
  Capability,
  CreateSandboxOptions,
  IsolateClientOptions,
  RunSandboxOptions,
  RunSandboxResult,
  SandboxInfo,
  SandboxMetrics,
  ListSandboxesOptions,
} from './types';

/**
 * Proto file path - relative to the package installation
 */
const PROTO_PATH = path.resolve(__dirname, '../proto/isolate.proto');

/**
 * Proto loader options for proper type handling
 */
const PROTO_LOADER_OPTIONS: protoLoader.Options = {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
};

/**
 * Isolate client for interacting with the Isolate gRPC server.
 *
 * Provides a high-level API for creating, running, and managing sandboxes.
 */
export class IsolateClient {
  private client: any;
  private address: string;
  private options: IsolateClientOptions;

  /**
   * Create a new Isolate client.
   *
   * @param address - Server address (e.g., 'localhost:50051')
   * @param options - Client options
   */
  constructor(address: string, options: IsolateClientOptions = {}) {
    this.address = address;
    this.options = options;

    // Load proto definition
    const packageDefinition = protoLoader.loadSync(
      options.protoPath || PROTO_PATH,
      PROTO_LOADER_OPTIONS
    );

    const protoDescriptor = grpc.loadPackageDefinition(packageDefinition);
    const isolate = (protoDescriptor as any).isolate.v1;

    // Create credentials
    const credentials = options.secure
      ? grpc.credentials.createSsl(
          options.rootCerts,
          options.privateKey,
          options.certChain
        )
      : grpc.credentials.createInsecure();

    // Create client
    this.client = new isolate.IsolateService(address, credentials);
  }

  /**
   * Create a new sandbox.
   *
   * @param module - WASM module bytes
   * @param options - Sandbox configuration options
   * @returns Promise resolving to sandbox creation result
   */
  async createSandbox(
    module: Buffer | Uint8Array,
    options: CreateSandboxOptions = {}
  ): Promise<{ sandboxId: string; moduleHash: string; creationTimeMs: number }> {
    return new Promise((resolve, reject) => {
      const request = {
        module: Buffer.from(module),
        config: {
          memory_limit: options.memoryLimit ?? 0,
          fuel_limit: options.fuelLimit ?? 0,
          wall_time_limit_secs: options.wallTimeLimitSecs ?? 0,
          cpu_time_limit_secs: options.cpuTimeLimitSecs ?? 0,
          capabilities: (options.capabilities ?? []).map((cap) => ({
            type: cap.type,
            value: cap.value ?? '',
          })),
          env: options.env ?? {},
          args: options.args ?? [],
        },
      };

      this.client.CreateSandbox(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to create sandbox'));
          return;
        }

        resolve({
          sandboxId: response.sandbox_id,
          moduleHash: response.module_hash,
          creationTimeMs: response.creation_time_ms,
        });
      });
    });
  }

  /**
   * Run a sandbox.
   *
   * @param sandboxId - ID of the sandbox to run
   * @param options - Run options
   * @returns Promise resolving to execution result
   */
  async runSandbox(
    sandboxId: string,
    options: RunSandboxOptions = {}
  ): Promise<RunSandboxResult> {
    return new Promise((resolve, reject) => {
      const request = {
        sandbox_id: sandboxId,
        input: options.input ? Buffer.from(options.input) : Buffer.alloc(0),
        entry_point: options.entryPoint ?? '_start',
      };

      this.client.RunSandbox(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to run sandbox'));
          return;
        }

        resolve({
          exitCode: response.exit_code,
          stdout: Buffer.from(response.stdout),
          stderr: Buffer.from(response.stderr),
          durationMs: response.duration_ms,
          resourceUsage: response.resource_usage
            ? {
                peakMemory: Number(response.resource_usage.peak_memory),
                fuelConsumed: Number(response.resource_usage.fuel_consumed),
                cpuTimeMs: response.resource_usage.cpu_time_ms,
                wallTimeMs: response.resource_usage.wall_time_ms,
                bytesRead: Number(response.resource_usage.bytes_read),
                bytesWritten: Number(response.resource_usage.bytes_written),
              }
            : undefined,
        });
      });
    });
  }

  /**
   * Get sandbox status and metrics.
   *
   * @param sandboxId - ID of the sandbox
   * @returns Promise resolving to sandbox info
   */
  async getSandbox(sandboxId: string): Promise<SandboxInfo> {
    return new Promise((resolve, reject) => {
      const request = { sandbox_id: sandboxId };

      this.client.GetSandbox(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to get sandbox'));
          return;
        }

        const sandbox = response.sandbox;
        resolve({
          id: sandbox.id,
          state: sandbox.state,
          moduleHash: sandbox.module_hash,
          createdAt: new Date(Number(sandbox.created_at) * 1000),
          ageSecs: sandbox.age_secs,
          metrics: sandbox.metrics
            ? this.parseMetrics(sandbox.metrics)
            : undefined,
        });
      });
    });
  }

  /**
   * Terminate a sandbox.
   *
   * @param sandboxId - ID of the sandbox to terminate
   * @returns Promise resolving to termination result
   */
  async terminateSandbox(
    sandboxId: string
  ): Promise<{ terminated: boolean; metrics?: SandboxMetrics }> {
    return new Promise((resolve, reject) => {
      const request = { sandbox_id: sandboxId };

      this.client.TerminateSandbox(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to terminate sandbox'));
          return;
        }

        resolve({
          terminated: response.terminated,
          metrics: response.metrics
            ? this.parseMetrics(response.metrics)
            : undefined,
        });
      });
    });
  }

  /**
   * List all sandboxes.
   *
   * @param options - List options
   * @returns Promise resolving to list of sandboxes
   */
  async listSandboxes(
    options: ListSandboxesOptions = {}
  ): Promise<{ sandboxes: SandboxInfo[]; total: number }> {
    return new Promise((resolve, reject) => {
      const request = {
        state_filter: options.stateFilter ?? '',
        limit: options.limit ?? 0,
        offset: options.offset ?? 0,
      };

      this.client.ListSandboxes(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to list sandboxes'));
          return;
        }

        resolve({
          sandboxes: (response.sandboxes ?? []).map((s: any) => ({
            id: s.id,
            state: s.state,
            moduleHash: s.module_hash,
            createdAt: new Date(Number(s.created_at) * 1000),
            ageSecs: s.age_secs,
            metrics: s.metrics ? this.parseMetrics(s.metrics) : undefined,
          })),
          total: response.total,
        });
      });
    });
  }

  /**
   * Get server metrics.
   *
   * @param format - Output format ('prometheus' or 'json')
   * @returns Promise resolving to metrics data
   */
  async getMetrics(format: 'prometheus' | 'json' = 'prometheus'): Promise<string> {
    return new Promise((resolve, reject) => {
      const request = { format };

      this.client.GetMetrics(request, (err: Error | null, response: any) => {
        if (err) {
          reject(this.wrapError(err, 'Failed to get metrics'));
          return;
        }

        resolve(response.data);
      });
    });
  }

  /**
   * Close the client connection.
   */
  close(): void {
    this.client.close();
  }

  /**
   * Convenience method to create, run, and terminate a sandbox in one call.
   *
   * @param module - WASM module bytes
   * @param options - Combined creation and run options
   * @returns Promise resolving to execution result
   */
  async execute(
    module: Buffer | Uint8Array,
    options: CreateSandboxOptions & RunSandboxOptions = {}
  ): Promise<RunSandboxResult> {
    const { sandboxId } = await this.createSandbox(module, options);

    try {
      const result = await this.runSandbox(sandboxId, options);
      return result;
    } finally {
      await this.terminateSandbox(sandboxId).catch(() => {
        // Ignore termination errors
      });
    }
  }

  private parseMetrics(metrics: any): SandboxMetrics {
    return {
      runCount: Number(metrics.run_count),
      successCount: Number(metrics.success_count),
      failureCount: Number(metrics.failure_count),
      totalRunDurationMs: metrics.total_run_duration_ms,
      lastRunDurationMs: metrics.last_run_duration_ms,
    };
  }

  private wrapError(err: Error, message: string): Error {
    const error = new Error(`${message}: ${err.message}`);
    error.cause = err;
    return error;
  }
}

/**
 * Create a new Isolate client.
 *
 * @param address - Server address (e.g., 'localhost:50051')
 * @param options - Client options
 * @returns New IsolateClient instance
 */
export function createClient(
  address: string,
  options?: IsolateClientOptions
): IsolateClient {
  return new IsolateClient(address, options);
}

// Default export
export default IsolateClient;
