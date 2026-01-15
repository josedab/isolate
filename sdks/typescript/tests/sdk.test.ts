/**
 * Unit tests for the Isolate TypeScript SDK.
 *
 * Tests cover model construction, capability helpers, Zod validation,
 * and error mapping — all without requiring a running gRPC server.
 *
 * Run with: npx vitest run tests/sdk.test.ts
 * Or: npx jest tests/sdk.test.ts
 */

import {
  Capabilities,
  CapabilitySchema,
  SandboxConfigSchema,
  CreateSandboxRequestSchema,
  RunSandboxRequestSchema,
  ResourceUsageSchema,
} from "../src/models";

import {
  IsolateError,
  ConnectionError,
  GrpcError,
  ValidationError,
  TimeoutError,
  ResourceExhaustedError,
  SandboxNotFoundError,
  PermissionDeniedError,
  mapGrpcError,
  GrpcStatusCode,
} from "../src/errors";

// ---------------------------------------------------------------------------
// Capability tests
// ---------------------------------------------------------------------------

describe("Capabilities", () => {
  test("stdout creates correct capability", () => {
    const cap = Capabilities.stdout();
    expect(cap.type).toBe("stdout");
    expect(cap.value).toBe("");
  });

  test("stderr creates correct capability", () => {
    const cap = Capabilities.stderr();
    expect(cap.type).toBe("stderr");
  });

  test("stdin creates correct capability", () => {
    const cap = Capabilities.stdin();
    expect(cap.type).toBe("stdin");
  });

  test("fsRead includes path", () => {
    const cap = Capabilities.fsRead("/data");
    expect(cap.type).toBe("fs_read");
    expect(cap.value).toBe("/data");
  });

  test("fsWrite includes path", () => {
    const cap = Capabilities.fsWrite("/output");
    expect(cap.type).toBe("fs_write");
    expect(cap.value).toBe("/output");
  });

  test("http includes host", () => {
    const cap = Capabilities.http("api.example.com");
    expect(cap.type).toBe("http");
    expect(cap.value).toBe("api.example.com");
  });

  test("dns creates correct capability", () => {
    const cap = Capabilities.dns();
    expect(cap.type).toBe("dns");
  });

  test("env includes variable name", () => {
    const cap = Capabilities.env("API_KEY");
    expect(cap.type).toBe("env");
    expect(cap.value).toBe("API_KEY");
  });
});

// ---------------------------------------------------------------------------
// Zod schema validation tests
// ---------------------------------------------------------------------------

describe("Schema Validation", () => {
  test("CapabilitySchema validates valid input", () => {
    const result = CapabilitySchema.safeParse({ type: "stdout", value: "" });
    expect(result.success).toBe(true);
  });

  test("CapabilitySchema rejects missing type", () => {
    const result = CapabilitySchema.safeParse({ value: "" });
    expect(result.success).toBe(false);
  });

  test("SandboxConfigSchema validates minimal config", () => {
    const result = SandboxConfigSchema.safeParse({
      capabilities: [],
    });
    expect(result.success).toBe(true);
  });

  test("CreateSandboxRequestSchema validates request", () => {
    const result = CreateSandboxRequestSchema.safeParse({
      module: Buffer.from([0x00, 0x61, 0x73, 0x6d]),
    });
    expect(result.success).toBe(true);
  });

  test("RunSandboxRequestSchema validates request", () => {
    const result = RunSandboxRequestSchema.safeParse({
      sandboxId: "sb-123",
    });
    expect(result.success).toBe(true);
  });

  test("ResourceUsageSchema validates usage", () => {
    const result = ResourceUsageSchema.safeParse({
      fuelConsumed: 50000,
      wallTimeMs: 150,
      peakMemory: 1048576,
    });
    expect(result.success).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Error tests
// ---------------------------------------------------------------------------

describe("Errors", () => {
  test("IsolateError is an Error instance", () => {
    const err = new IsolateError("test");
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe("test");
  });

  test("ConnectionError extends IsolateError", () => {
    const err = new ConnectionError("unreachable");
    expect(err).toBeInstanceOf(IsolateError);
    expect(err).toBeInstanceOf(Error);
  });

  test("ValidationError extends IsolateError", () => {
    const err = new ValidationError("bad input");
    expect(err).toBeInstanceOf(IsolateError);
  });

  test("TimeoutError extends IsolateError", () => {
    const err = new TimeoutError("timed out");
    expect(err).toBeInstanceOf(IsolateError);
  });

  test("ResourceExhaustedError extends IsolateError", () => {
    const err = new ResourceExhaustedError("OOM");
    expect(err).toBeInstanceOf(IsolateError);
  });

  test("SandboxNotFoundError extends IsolateError", () => {
    const err = new SandboxNotFoundError("sb-123");
    expect(err).toBeInstanceOf(IsolateError);
  });

  test("PermissionDeniedError extends IsolateError", () => {
    const err = new PermissionDeniedError("forbidden");
    expect(err).toBeInstanceOf(IsolateError);
  });

  test("GrpcError includes status code", () => {
    const err = new GrpcError("failed", GrpcStatusCode.INTERNAL);
    expect(err.code).toBe(GrpcStatusCode.INTERNAL);
  });

  test("mapGrpcError handles non-Error input", () => {
    const err = mapGrpcError("string error");
    expect(err).toBeInstanceOf(IsolateError);
  });
});
