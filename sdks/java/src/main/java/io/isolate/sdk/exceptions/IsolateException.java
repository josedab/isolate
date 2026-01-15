package io.isolate.sdk.exceptions;

import io.grpc.Status;

/**
 * Base exception for all Isolate SDK errors.
 *
 * <p>Every exception thrown by the SDK extends this class, making it possible to
 * catch all SDK-originated failures in a single catch block. The exception
 * carries the operation name and, when available, the gRPC status code that
 * triggered the failure.</p>
 *
 * <pre>{@code
 * try {
 *     client.createSandbox(wasmBytes, config);
 * } catch (IsolateException e) {
 *     System.err.println("Operation failed: " + e.getOperation());
 *     System.err.println("gRPC code: " + e.getGrpcStatusCode());
 * }
 * }</pre>
 */
public class IsolateException extends Exception {

    private static final long serialVersionUID = 1L;

    private final String operation;
    private final Status.Code grpcStatusCode;

    /**
     * Creates a new IsolateException.
     *
     * @param message human-readable error description
     */
    public IsolateException(String message) {
        this(message, null, null, null);
    }

    /**
     * Creates a new IsolateException with a cause.
     *
     * @param message human-readable error description
     * @param cause   the underlying cause
     */
    public IsolateException(String message, Throwable cause) {
        this(message, null, null, cause);
    }

    /**
     * Creates a new IsolateException with full context.
     *
     * @param message        human-readable error description
     * @param operation      the SDK operation that failed (e.g., "createSandbox")
     * @param grpcStatusCode the gRPC status code, or {@code null} if not applicable
     * @param cause          the underlying cause, or {@code null}
     */
    public IsolateException(String message, String operation, Status.Code grpcStatusCode, Throwable cause) {
        super(message, cause);
        this.operation = operation;
        this.grpcStatusCode = grpcStatusCode;
    }

    /**
     * Returns the SDK operation that failed.
     *
     * @return the operation name (e.g., "createSandbox"), or {@code null} if unknown
     */
    public String getOperation() {
        return operation;
    }

    /**
     * Returns the gRPC status code associated with this error.
     *
     * @return the gRPC status code, or {@code null} if the error did not originate from gRPC
     */
    public Status.Code getGrpcStatusCode() {
        return grpcStatusCode;
    }

    @Override
    public String toString() {
        StringBuilder sb = new StringBuilder(getClass().getSimpleName());
        sb.append(": ").append(getMessage());
        if (operation != null) {
            sb.append(" [operation=").append(operation).append("]");
        }
        if (grpcStatusCode != null) {
            sb.append(" [grpcCode=").append(grpcStatusCode).append("]");
        }
        return sb.toString();
    }
}
