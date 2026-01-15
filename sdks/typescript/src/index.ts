/**
 * @isolate/sdk -- TypeScript client for the Isolate gRPC sandbox service.
 *
 * @example
 * ```ts
 * import { IsolateClient, Capabilities } from "@isolate/sdk";
 * import { readFileSync } from "fs";
 *
 * const client = new IsolateClient("localhost:50051");
 *
 * const result = await client.execute(readFileSync("module.wasm"), {
 *   memoryLimit: 64 * 1024 * 1024,
 *   capabilities: [Capabilities.stdout()],
 * });
 *
 * console.log("Exit:", result.exitCode);
 * console.log("Output:", Buffer.from(result.stdout).toString());
 *
 * client.close();
 * ```
 *
 * @packageDocumentation
 */

// ---- Client ---------------------------------------------------------------
export { IsolateClient, createClient } from "./client";

// ---- Models / Types -------------------------------------------------------
export { Capabilities } from "./models";

export type {
  Capability,
  SandboxConfig,
  ResourceUsage,
  SandboxMetrics,
  SandboxInfo,
  CreateSandboxRequest,
  CreateSandboxResponse,
  RunSandboxRequest,
  RunSandboxResponse,
  GetSandboxRequest,
  GetSandboxResponse,
  TerminateSandboxRequest,
  TerminateSandboxResponse,
  ListSandboxesRequest,
  ListSandboxesResponse,
  StreamOutputRequest,
  OutputChunk,
  GetMetricsRequest,
  GetMetricsResponse,
  TlsOptions,
  IsolateClientOptions,
} from "./models";

// ---- Validation schemas ---------------------------------------------------
export {
  CapabilitySchema,
  SandboxConfigSchema,
  ResourceUsageSchema,
  SandboxMetricsSchema,
  SandboxInfoSchema,
  CreateSandboxRequestSchema,
  RunSandboxRequestSchema,
  GetSandboxRequestSchema,
  TerminateSandboxRequestSchema,
  ListSandboxesRequestSchema,
  StreamOutputRequestSchema,
  GetMetricsRequestSchema,
} from "./models";

// ---- Errors ---------------------------------------------------------------
export {
  IsolateError,
  ConnectionError,
  GrpcError,
  ValidationError,
  TimeoutError,
  ResourceExhaustedError,
  SandboxNotFoundError,
  PermissionDeniedError,
  GrpcStatusCode,
  mapGrpcError,
} from "./errors";
