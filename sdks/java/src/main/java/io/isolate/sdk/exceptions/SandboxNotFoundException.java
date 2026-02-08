package io.isolate.sdk.exceptions;

import io.grpc.Status;

/**
 * Thrown when a referenced sandbox does not exist on the server.
 *
 * <p>This typically maps to a gRPC {@code NOT_FOUND} status. The sandbox ID
 * that was not found is available via {@link #getSandboxId()}.</p>
 *
 * <pre>{@code
 * try {
 *     client.getSandbox("nonexistent-id");
 * } catch (SandboxNotFoundException e) {
 *     System.err.println("Sandbox not found: " + e.getSandboxId());
 * }
 * }</pre>
 */
public class SandboxNotFoundException extends IsolateException {

    private static final long serialVersionUID = 1L;

    private final String sandboxId;

    /**
     * Creates a new SandboxNotFoundException.
     *
     * @param sandboxId the ID of the sandbox that was not found
     */
    public SandboxNotFoundException(String sandboxId) {
        this(sandboxId, null, null);
    }

    /**
     * Creates a new SandboxNotFoundException with operation context.
     *
     * @param sandboxId the ID of the sandbox that was not found
     * @param operation the SDK operation that failed
     * @param cause     the underlying cause, or {@code null}
     */
    public SandboxNotFoundException(String sandboxId, String operation, Throwable cause) {
        super(
                "Sandbox not found: " + sandboxId,
                operation,
                Status.Code.NOT_FOUND,
                cause
        );
        this.sandboxId = sandboxId;
    }

    /**
     * Returns the ID of the sandbox that was not found.
     *
     * @return the sandbox ID
     */
    public String getSandboxId() {
        return sandboxId;
    }
}
