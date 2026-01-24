package io.isolate.sdk.exceptions;

import io.grpc.Status;

/**
 * Thrown when a sandbox execution fails.
 *
 * <p>This exception covers failures during sandbox runs, including resource
 * exhaustion, timeout, and permission denied errors. The sandbox ID and,
 * when available, the exit code from the failed execution are accessible.</p>
 *
 * <pre>{@code
 * try {
 *     client.runSandbox(sandboxId, input);
 * } catch (SandboxExecutionException e) {
 *     System.err.println("Execution failed for sandbox: " + e.getSandboxId());
 *     e.getExitCode().ifPresent(code ->
 *         System.err.println("Exit code: " + code));
 * }
 * }</pre>
 */
public class SandboxExecutionException extends IsolateException {

    private static final long serialVersionUID = 1L;

    private final String sandboxId;
    private final Integer exitCode;

    /**
     * Creates a new SandboxExecutionException.
     *
     * @param message   human-readable error description
     * @param sandboxId the ID of the sandbox that failed
     */
    public SandboxExecutionException(String message, String sandboxId) {
        this(message, sandboxId, null, null, null, null);
    }

    /**
     * Creates a new SandboxExecutionException with full context.
     *
     * @param message        human-readable error description
     * @param sandboxId      the ID of the sandbox that failed
     * @param exitCode       the exit code from the execution, or {@code null} if unavailable
     * @param operation      the SDK operation that failed
     * @param grpcStatusCode the gRPC status code, or {@code null}
     * @param cause          the underlying cause, or {@code null}
     */
    public SandboxExecutionException(
            String message,
            String sandboxId,
            Integer exitCode,
            String operation,
            Status.Code grpcStatusCode,
            Throwable cause) {
        super(message, operation, grpcStatusCode, cause);
        this.sandboxId = sandboxId;
        this.exitCode = exitCode;
    }

    /**
     * Returns the ID of the sandbox that failed.
     *
     * @return the sandbox ID
     */
    public String getSandboxId() {
        return sandboxId;
    }

    /**
     * Returns the exit code from the failed execution, if available.
     *
     * @return an {@link java.util.Optional} containing the exit code, or empty
     */
    public java.util.Optional<Integer> getExitCode() {
        return java.util.Optional.ofNullable(exitCode);
    }
}
