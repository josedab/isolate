/**
 * Custom error hierarchy for the Isolate SDK.
 *
 * All errors thrown by the SDK extend {@link IsolateError}, making it
 * straightforward to distinguish SDK errors from other runtime errors.
 */

/** gRPC status codes relevant to the Isolate service. */
export enum GrpcStatusCode {
  OK = 0,
  CANCELLED = 1,
  UNKNOWN = 2,
  INVALID_ARGUMENT = 3,
  DEADLINE_EXCEEDED = 4,
  NOT_FOUND = 5,
  ALREADY_EXISTS = 6,
  PERMISSION_DENIED = 7,
  RESOURCE_EXHAUSTED = 8,
  FAILED_PRECONDITION = 9,
  ABORTED = 10,
  UNIMPLEMENTED = 12,
  INTERNAL = 13,
  UNAVAILABLE = 14,
  UNAUTHENTICATED = 16,
}

/**
 * Base error class for all Isolate SDK errors.
 *
 * Every error produced by the SDK extends this class. Consumers can catch
 * `IsolateError` to handle any SDK-originated failure in a single branch.
 */
export class IsolateError extends Error {
  public readonly code: string;

  constructor(message: string, code: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "IsolateError";
    this.code = code;
  }
}

/**
 * Thrown when the client cannot establish or maintain a connection to the
 * Isolate gRPC server.
 */
export class ConnectionError extends IsolateError {
  public readonly address: string;

  constructor(message: string, address: string, options?: ErrorOptions) {
    super(message, "CONNECTION_ERROR", options);
    this.name = "ConnectionError";
    this.address = address;
  }
}

/**
 * Thrown when the server returns a gRPC error. Includes the gRPC status code
 * and any details provided by the server.
 */
export class GrpcError extends IsolateError {
  public readonly grpcCode: GrpcStatusCode;
  public readonly details: string;

  constructor(
    message: string,
    grpcCode: GrpcStatusCode,
    details: string,
    options?: ErrorOptions,
  ) {
    super(message, "GRPC_ERROR", options);
    this.name = "GrpcError";
    this.grpcCode = grpcCode;
    this.details = details;
  }
}

/**
 * Thrown when a request or configuration value fails validation before being
 * sent to the server.
 */
export class ValidationError extends IsolateError {
  public readonly field: string;

  constructor(message: string, field: string, options?: ErrorOptions) {
    super(message, "VALIDATION_ERROR", options);
    this.name = "ValidationError";
    this.field = field;
  }
}

/**
 * Thrown when a sandbox execution exceeds its configured wall-time or
 * deadline.
 */
export class TimeoutError extends IsolateError {
  public readonly timeoutMs: number;

  constructor(message: string, timeoutMs: number, options?: ErrorOptions) {
    super(message, "TIMEOUT_ERROR", options);
    this.name = "TimeoutError";
    this.timeoutMs = timeoutMs;
  }
}

/**
 * Thrown when a sandbox exceeds a resource limit (memory, fuel, I/O).
 */
export class ResourceExhaustedError extends IsolateError {
  public readonly resource: string;

  constructor(message: string, resource: string, options?: ErrorOptions) {
    super(message, "RESOURCE_EXHAUSTED", options);
    this.name = "ResourceExhaustedError";
    this.resource = resource;
  }
}

/**
 * Thrown when a referenced sandbox does not exist.
 */
export class SandboxNotFoundError extends IsolateError {
  public readonly sandboxId: string;

  constructor(sandboxId: string, options?: ErrorOptions) {
    super(`Sandbox not found: ${sandboxId}`, "SANDBOX_NOT_FOUND", options);
    this.name = "SandboxNotFoundError";
    this.sandboxId = sandboxId;
  }
}

/**
 * Thrown when the sandbox lacks a required capability for the requested
 * operation.
 */
export class PermissionDeniedError extends IsolateError {
  constructor(message: string, options?: ErrorOptions) {
    super(message, "PERMISSION_DENIED", options);
    this.name = "PermissionDeniedError";
  }
}

/**
 * Maps a raw gRPC error (from @grpc/grpc-js) to the appropriate typed SDK
 * error. If the error does not match a known gRPC shape it is wrapped in a
 * generic {@link IsolateError}.
 */
export function mapGrpcError(err: unknown): IsolateError {
  if (err instanceof IsolateError) {
    return err;
  }

  const grpcErr = err as {
    code?: number;
    message?: string;
    details?: string;
    metadata?: unknown;
  };

  const code = grpcErr.code ?? GrpcStatusCode.UNKNOWN;
  const message = grpcErr.message ?? "Unknown gRPC error";
  const details = grpcErr.details ?? "";

  switch (code) {
    case GrpcStatusCode.INVALID_ARGUMENT:
      return new ValidationError(message, "request", { cause: err });

    case GrpcStatusCode.DEADLINE_EXCEEDED:
      return new TimeoutError(message, 0, { cause: err });

    case GrpcStatusCode.NOT_FOUND:
      return new SandboxNotFoundError(details || "unknown", { cause: err });

    case GrpcStatusCode.PERMISSION_DENIED:
      return new PermissionDeniedError(message, { cause: err });

    case GrpcStatusCode.RESOURCE_EXHAUSTED:
      return new ResourceExhaustedError(message, "unknown", { cause: err });

    case GrpcStatusCode.UNAVAILABLE:
      return new ConnectionError(message, "", { cause: err });

    default:
      return new GrpcError(message, code, details, { cause: err });
  }
}
