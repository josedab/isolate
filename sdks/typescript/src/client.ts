/**
 * Main Isolate gRPC client.
 *
 * Provides a Promise-based async/await API for every RPC defined in the
 * `isolate.v1.IsolateService` proto, plus a convenience `execute` helper
 * that combines create + run + terminate into a single call.
 */

import * as grpc from "@grpc/grpc-js";
import * as protoLoader from "@grpc/proto-loader";
import * as path from "path";

import {
  type Capability,
  type CreateSandboxResponse,
  CreateSandboxRequestSchema,
  type GetMetricsResponse,
  type GetSandboxResponse,
  type IsolateClientOptions,
  type ListSandboxesRequest,
  ListSandboxesRequestSchema,
  type ListSandboxesResponse,
  type OutputChunk,
  type ResourceUsage,
  type RunSandboxResponse,
  RunSandboxRequestSchema,
  type SandboxConfig,
  type SandboxInfo,
  type SandboxMetrics,
  type StreamOutputRequest,
  StreamOutputRequestSchema,
  type TerminateSandboxResponse,
  type TlsOptions,
} from "./models";

import {
  ConnectionError,
  IsolateError,
  ValidationError,
  mapGrpcError,
} from "./errors";

// ---------------------------------------------------------------------------
// Proto loading types
// ---------------------------------------------------------------------------

/**
 * Shape of the dynamically-loaded gRPC service client. The proto-loader
 * produces a generic object; we cast through this interface for clarity.
 */
interface IsolateServiceClient {
  createSandbox(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  runSandbox(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  getSandbox(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  terminateSandbox(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  listSandboxes(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  streamOutput(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
  ): grpc.ClientReadableStream<unknown>;

  getMetrics(
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ): void;

  close(): void;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

const DEFAULT_TIMEOUT_MS = 30_000;

const PROTO_PATH = path.resolve(__dirname, "..", "..", "..", "proto", "isolate.proto");

/** Build gRPC channel credentials from the TLS options. */
function buildCredentials(tls?: TlsOptions): grpc.ChannelCredentials {
  if (!tls || !tls.secure) {
    return grpc.credentials.createInsecure();
  }

  return grpc.credentials.createSsl(
    tls.rootCerts ?? null,
    tls.privateKey ?? null,
    tls.certChain ?? null,
  );
}

/** Convert camelCase keys from the proto-loader response to our SDK types. */
function toResourceUsage(raw: Record<string, unknown>): ResourceUsage {
  return {
    peakMemory: Number(raw.peakMemory ?? raw.peak_memory ?? 0),
    fuelConsumed: Number(raw.fuelConsumed ?? raw.fuel_consumed ?? 0),
    cpuTimeMs: Number(raw.cpuTimeMs ?? raw.cpu_time_ms ?? 0),
    wallTimeMs: Number(raw.wallTimeMs ?? raw.wall_time_ms ?? 0),
    bytesRead: Number(raw.bytesRead ?? raw.bytes_read ?? 0),
    bytesWritten: Number(raw.bytesWritten ?? raw.bytes_written ?? 0),
  };
}

function toSandboxMetrics(
  raw: Record<string, unknown> | undefined,
): SandboxMetrics | undefined {
  if (!raw) return undefined;
  return {
    runCount: Number(raw.runCount ?? raw.run_count ?? 0),
    successCount: Number(raw.successCount ?? raw.success_count ?? 0),
    failureCount: Number(raw.failureCount ?? raw.failure_count ?? 0),
    totalRunDurationMs: Number(
      raw.totalRunDurationMs ?? raw.total_run_duration_ms ?? 0,
    ),
    lastRunDurationMs: Number(
      raw.lastRunDurationMs ?? raw.last_run_duration_ms ?? 0,
    ),
  };
}

function toSandboxInfo(raw: Record<string, unknown>): SandboxInfo {
  return {
    id: String(raw.id ?? ""),
    state: String(raw.state ?? ""),
    moduleHash: String(raw.moduleHash ?? raw.module_hash ?? ""),
    createdAt: Number(raw.createdAt ?? raw.created_at ?? 0),
    ageSecs: Number(raw.ageSecs ?? raw.age_secs ?? 0),
    metrics: toSandboxMetrics(
      (raw.metrics as Record<string, unknown>) ?? undefined,
    ),
  };
}

/** Build the protobuf Capability message from our SDK type. */
function toProtoCapability(cap: Capability): { type: string; value: string } {
  return { type: cap.type, value: cap.value };
}

/** Build the protobuf SandboxConfig message from our SDK type. */
function toProtoConfig(
  config?: SandboxConfig,
): Record<string, unknown> | undefined {
  if (!config) return undefined;
  return {
    memory_limit: config.memoryLimit ?? 0,
    fuel_limit: config.fuelLimit ?? 0,
    wall_time_limit_secs: config.wallTimeLimitSecs ?? 0,
    cpu_time_limit_secs: config.cpuTimeLimitSecs ?? 0,
    capabilities: (config.capabilities ?? []).map(toProtoCapability),
    env: config.env ?? {},
    args: config.args ?? [],
  };
}

/** Wrap a callback-based gRPC call in a Promise. */
function unaryCall<T>(
  method: (
    request: unknown,
    metadata: grpc.Metadata,
    options: Partial<grpc.CallOptions>,
    callback: (err: grpc.ServiceError | null, response: unknown) => void,
  ) => void,
  request: unknown,
  metadata: grpc.Metadata,
  options: Partial<grpc.CallOptions>,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    method(request, metadata, options, (err, response) => {
      if (err) {
        reject(mapGrpcError(err));
      } else {
        resolve(response as T);
      }
    });
  });
}

// ---------------------------------------------------------------------------
// IsolateClient
// ---------------------------------------------------------------------------

/**
 * A client for the Isolate gRPC sandbox service.
 *
 * ```ts
 * const client = new IsolateClient("localhost:50051");
 * const { sandboxId } = await client.createSandbox(wasmBytes, { ... });
 * const result = await client.runSandbox(sandboxId);
 * await client.terminateSandbox(sandboxId);
 * client.close();
 * ```
 */
export class IsolateClient {
  private readonly grpcClient: IsolateServiceClient;
  private readonly defaultTimeoutMs: number;
  private closed = false;

  /**
   * Create a new client connected to the given address.
   *
   * @param address - gRPC server address in `host:port` form.
   * @param options - Optional client configuration (TLS, timeouts, etc.).
   */
  constructor(address: string, options?: IsolateClientOptions) {
    if (!address || address.trim().length === 0) {
      throw new ValidationError("Server address must not be empty", "address");
    }

    this.defaultTimeoutMs = options?.defaultTimeoutMs ?? DEFAULT_TIMEOUT_MS;

    const packageDefinition = protoLoader.loadSync(PROTO_PATH, {
      keepCase: false,
      longs: Number,
      enums: String,
      defaults: true,
      oneofs: true,
    });

    const proto = grpc.loadPackageDefinition(packageDefinition) as Record<
      string,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      any
    >;

    const ServiceConstructor = proto.isolate?.v1?.IsolateService;
    if (!ServiceConstructor) {
      throw new ConnectionError(
        "Failed to load IsolateService from proto definition. " +
          "Ensure the proto file is accessible at: " +
          PROTO_PATH,
        address,
      );
    }

    const credentials = buildCredentials(options?.tls);
    const channelOptions = options?.channelOptions ?? {};

    this.grpcClient = new ServiceConstructor(
      address,
      credentials,
      channelOptions,
    ) as IsolateServiceClient;
  }

  // -----------------------------------------------------------------------
  // Lifecycle helpers
  // -----------------------------------------------------------------------

  /** Ensure the client has not been closed. */
  private assertOpen(): void {
    if (this.closed) {
      throw new IsolateError(
        "Client has been closed",
        "CLIENT_CLOSED",
      );
    }
  }

  /** Build CallOptions with a deadline derived from the configured timeout. */
  private callOptions(timeoutMs?: number): Partial<grpc.CallOptions> {
    const ms = timeoutMs ?? this.defaultTimeoutMs;
    return { deadline: new Date(Date.now() + ms) };
  }

  // -----------------------------------------------------------------------
  // RPC methods
  // -----------------------------------------------------------------------

  /**
   * Create a new sandbox from a WASM module.
   *
   * @param module - WASM module bytes (Uint8Array or Buffer).
   * @param config - Optional sandbox configuration.
   * @param timeoutMs - Per-call timeout override.
   */
  async createSandbox(
    module: Uint8Array,
    config?: SandboxConfig,
    timeoutMs?: number,
  ): Promise<CreateSandboxResponse> {
    this.assertOpen();

    const validation = CreateSandboxRequestSchema.safeParse({ module, config });
    if (!validation.success) {
      const issue = validation.error.issues[0];
      throw new ValidationError(
        issue?.message ?? "Invalid createSandbox request",
        issue?.path.join(".") ?? "unknown",
      );
    }

    const request = {
      module: Buffer.from(module),
      config: toProtoConfig(config),
    };

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.createSandbox.bind(this.grpcClient),
      request,
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    return {
      sandboxId: String(raw.sandboxId ?? raw.sandbox_id ?? ""),
      moduleHash: String(raw.moduleHash ?? raw.module_hash ?? ""),
      creationTimeMs: Number(raw.creationTimeMs ?? raw.creation_time_ms ?? 0),
    };
  }

  /**
   * Run a previously created sandbox.
   *
   * @param sandboxId - The sandbox to run.
   * @param input - Optional input data (fed to stdin).
   * @param entryPoint - Function to invoke (defaults to "_start").
   * @param timeoutMs - Per-call timeout override.
   */
  async runSandbox(
    sandboxId: string,
    input?: Uint8Array | string,
    entryPoint?: string,
    timeoutMs?: number,
  ): Promise<RunSandboxResponse> {
    this.assertOpen();

    const inputBytes =
      input === undefined
        ? undefined
        : typeof input === "string"
          ? new Uint8Array(Buffer.from(input, "utf-8"))
          : input;

    const validation = RunSandboxRequestSchema.safeParse({
      sandboxId,
      input: inputBytes,
      entryPoint,
    });
    if (!validation.success) {
      const issue = validation.error.issues[0];
      throw new ValidationError(
        issue?.message ?? "Invalid runSandbox request",
        issue?.path.join(".") ?? "unknown",
      );
    }

    const request: Record<string, unknown> = {
      sandbox_id: sandboxId,
    };
    if (inputBytes) {
      request.input = Buffer.from(inputBytes);
    }
    if (entryPoint) {
      request.entry_point = entryPoint;
    }

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.runSandbox.bind(this.grpcClient),
      request,
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    const rawStdout = raw.stdout;
    const rawStderr = raw.stderr;

    return {
      exitCode: Number(raw.exitCode ?? raw.exit_code ?? 0),
      stdout:
        rawStdout instanceof Uint8Array
          ? rawStdout
          : Buffer.from(rawStdout as ArrayBuffer | string ?? ""),
      stderr:
        rawStderr instanceof Uint8Array
          ? rawStderr
          : Buffer.from(rawStderr as ArrayBuffer | string ?? ""),
      durationMs: Number(raw.durationMs ?? raw.duration_ms ?? 0),
      resourceUsage: raw.resourceUsage ?? raw.resource_usage
        ? toResourceUsage(
            (raw.resourceUsage ?? raw.resource_usage) as Record<string, unknown>,
          )
        : undefined,
    };
  }

  /**
   * Retrieve the current status of a sandbox.
   *
   * @param sandboxId - The sandbox to query.
   * @param timeoutMs - Per-call timeout override.
   */
  async getSandbox(
    sandboxId: string,
    timeoutMs?: number,
  ): Promise<GetSandboxResponse> {
    this.assertOpen();

    if (!sandboxId || sandboxId.trim().length === 0) {
      throw new ValidationError("sandboxId must not be empty", "sandboxId");
    }

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.getSandbox.bind(this.grpcClient),
      { sandbox_id: sandboxId },
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    const rawSandbox = (raw.sandbox ?? {}) as Record<string, unknown>;
    return {
      sandbox: toSandboxInfo(rawSandbox),
    };
  }

  /**
   * Terminate a running sandbox and retrieve its final metrics.
   *
   * @param sandboxId - The sandbox to terminate.
   * @param timeoutMs - Per-call timeout override.
   */
  async terminateSandbox(
    sandboxId: string,
    timeoutMs?: number,
  ): Promise<TerminateSandboxResponse> {
    this.assertOpen();

    if (!sandboxId || sandboxId.trim().length === 0) {
      throw new ValidationError("sandboxId must not be empty", "sandboxId");
    }

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.terminateSandbox.bind(this.grpcClient),
      { sandbox_id: sandboxId },
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    return {
      terminated: Boolean(raw.terminated),
      metrics: toSandboxMetrics(
        (raw.metrics as Record<string, unknown>) ?? undefined,
      ),
    };
  }

  /**
   * List sandboxes with optional filtering and pagination.
   *
   * @param options - Filter, limit, and offset options.
   * @param timeoutMs - Per-call timeout override.
   */
  async listSandboxes(
    options?: ListSandboxesRequest,
    timeoutMs?: number,
  ): Promise<ListSandboxesResponse> {
    this.assertOpen();

    if (options) {
      const validation = ListSandboxesRequestSchema.safeParse(options);
      if (!validation.success) {
        const issue = validation.error.issues[0];
        throw new ValidationError(
          issue?.message ?? "Invalid listSandboxes request",
          issue?.path.join(".") ?? "unknown",
        );
      }
    }

    const request: Record<string, unknown> = {};
    if (options?.stateFilter) {
      request.state_filter = options.stateFilter;
    }
    if (options?.limit !== undefined) {
      request.limit = options.limit;
    }
    if (options?.offset !== undefined) {
      request.offset = options.offset;
    }

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.listSandboxes.bind(this.grpcClient),
      request,
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    const rawSandboxes = (raw.sandboxes ?? []) as Record<string, unknown>[];
    return {
      sandboxes: rawSandboxes.map(toSandboxInfo),
      total: Number(raw.total ?? 0),
    };
  }

  /**
   * Stream real-time output from a running sandbox.
   *
   * Returns an `AsyncIterable<OutputChunk>` that yields output chunks as
   * they arrive. The iterator completes when the sandbox finishes or the
   * stream is cancelled.
   *
   * @param options - Stream configuration (sandbox ID, stdout/stderr flags).
   */
  streamOutput(
    options: StreamOutputRequest,
  ): AsyncIterable<OutputChunk> {
    this.assertOpen();

    const validation = StreamOutputRequestSchema.safeParse(options);
    if (!validation.success) {
      const issue = validation.error.issues[0];
      throw new ValidationError(
        issue?.message ?? "Invalid streamOutput request",
        issue?.path.join(".") ?? "unknown",
      );
    }

    const request: Record<string, unknown> = {
      sandbox_id: options.sandboxId,
      follow_stdout: options.followStdout ?? true,
      follow_stderr: options.followStderr ?? true,
    };

    const stream = this.grpcClient.streamOutput(
      request,
      new grpc.Metadata(),
      {},
    );

    return new OutputStreamIterator(stream);
  }

  /**
   * Retrieve server metrics in the specified format.
   *
   * @param format - "prometheus" or "json". Defaults to "prometheus".
   * @param timeoutMs - Per-call timeout override.
   */
  async getMetrics(
    format?: string,
    timeoutMs?: number,
  ): Promise<GetMetricsResponse> {
    this.assertOpen();

    const raw = await unaryCall<Record<string, unknown>>(
      this.grpcClient.getMetrics.bind(this.grpcClient),
      { format: format ?? "prometheus" },
      new grpc.Metadata(),
      this.callOptions(timeoutMs),
    );

    return {
      data: String(raw.data ?? ""),
    };
  }

  // -----------------------------------------------------------------------
  // Convenience: one-shot execute
  // -----------------------------------------------------------------------

  /**
   * Convenience method that creates a sandbox, runs it, and terminates it
   * in a single call.
   *
   * @param module - WASM module bytes.
   * @param config - Sandbox configuration.
   * @param input - Optional input bytes or string.
   * @param timeoutMs - Per-call timeout override (applied to each sub-call).
   */
  async execute(
    module: Uint8Array,
    config?: SandboxConfig & { input?: Uint8Array | string },
    timeoutMs?: number,
  ): Promise<RunSandboxResponse & { sandboxId: string }> {
    const { input, ...sandboxConfig } = config ?? {};

    const createResult = await this.createSandbox(
      module,
      sandboxConfig,
      timeoutMs,
    );

    try {
      const runResult = await this.runSandbox(
        createResult.sandboxId,
        input as Uint8Array | string | undefined,
        undefined,
        timeoutMs,
      );

      return {
        ...runResult,
        sandboxId: createResult.sandboxId,
      };
    } finally {
      // Best-effort cleanup; do not mask the original error.
      try {
        await this.terminateSandbox(createResult.sandboxId, timeoutMs);
      } catch {
        // Intentionally swallowed so the caller sees the run error.
      }
    }
  }

  // -----------------------------------------------------------------------
  // Cleanup
  // -----------------------------------------------------------------------

  /**
   * Close the underlying gRPC channel and release resources. The client
   * must not be used after calling this method.
   */
  close(): void {
    if (!this.closed) {
      this.closed = true;
      this.grpcClient.close();
    }
  }
}

// ---------------------------------------------------------------------------
// OutputStreamIterator
// ---------------------------------------------------------------------------

/**
 * Adapts a gRPC server-streaming response into an `AsyncIterable` that
 * can be consumed with `for await ... of`.
 */
class OutputStreamIterator implements AsyncIterable<OutputChunk> {
  private readonly stream: grpc.ClientReadableStream<unknown>;

  constructor(stream: grpc.ClientReadableStream<unknown>) {
    this.stream = stream;
  }

  [Symbol.asyncIterator](): AsyncIterator<OutputChunk> {
    const stream = this.stream;

    // We buffer incoming events and resolve waiters in order.
    const buffer: Array<
      | { done: false; value: OutputChunk }
      | { done: true; value: undefined }
      | { error: IsolateError }
    > = [];
    let finished = false;
    let waiting: {
      resolve: (
        result: IteratorResult<OutputChunk>,
      ) => void;
      reject: (err: unknown) => void;
    } | null = null;

    function push(
      item:
        | { done: false; value: OutputChunk }
        | { done: true; value: undefined }
        | { error: IsolateError },
    ): void {
      if (waiting) {
        const w = waiting;
        waiting = null;
        if ("error" in item) {
          w.reject(item.error);
        } else if (item.done) {
          w.resolve({ done: true, value: undefined });
        } else {
          w.resolve({ done: false, value: item.value });
        }
      } else {
        buffer.push(item);
      }
    }

    stream.on("data", (raw: Record<string, unknown>) => {
      const rawData = raw.data;
      const chunk: OutputChunk = {
        stream: String(raw.stream ?? ""),
        data:
          rawData instanceof Uint8Array
            ? rawData
            : Buffer.from((rawData as string) ?? ""),
        timestamp: Number(raw.timestamp ?? 0),
      };
      push({ done: false, value: chunk });
    });

    stream.on("end", () => {
      finished = true;
      push({ done: true, value: undefined });
    });

    stream.on("error", (err: unknown) => {
      finished = true;
      push({ error: mapGrpcError(err) });
    });

    return {
      next(): Promise<IteratorResult<OutputChunk>> {
        // Drain the buffer first.
        const item = buffer.shift();
        if (item) {
          if ("error" in item) {
            return Promise.reject(item.error);
          }
          if (item.done) {
            return Promise.resolve({ done: true, value: undefined });
          }
          return Promise.resolve({ done: false, value: item.value });
        }

        if (finished) {
          return Promise.resolve({ done: true, value: undefined });
        }

        // Wait for the next event.
        return new Promise<IteratorResult<OutputChunk>>((resolve, reject) => {
          waiting = { resolve, reject };
        });
      },

      return(): Promise<IteratorResult<OutputChunk>> {
        stream.cancel();
        finished = true;
        return Promise.resolve({ done: true, value: undefined });
      },
    };
  }
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/**
 * Convenience factory for creating an {@link IsolateClient}.
 *
 * ```ts
 * const client = createClient("localhost:50051", { defaultTimeoutMs: 10_000 });
 * ```
 */
export function createClient(
  address: string,
  options?: IsolateClientOptions,
): IsolateClient {
  return new IsolateClient(address, options);
}
