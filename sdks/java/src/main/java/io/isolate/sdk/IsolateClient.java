package io.isolate.sdk;

import com.google.protobuf.ByteString;
import io.grpc.ManagedChannel;
import io.grpc.ManagedChannelBuilder;
import io.grpc.Status;
import io.grpc.StatusRuntimeException;
import io.grpc.netty.shaded.io.grpc.netty.GrpcSslContexts;
import io.grpc.netty.shaded.io.grpc.netty.NettyChannelBuilder;
import io.grpc.netty.shaded.io.netty.handler.ssl.SslContext;
import io.grpc.netty.shaded.io.netty.handler.ssl.SslContextBuilder;
import io.isolate.sdk.exceptions.IsolateException;
import io.isolate.sdk.exceptions.SandboxExecutionException;
import io.isolate.sdk.exceptions.SandboxNotFoundException;
import io.isolate.sdk.models.ResourceUsage;
import io.isolate.sdk.models.RunResult;
import io.isolate.sdk.models.SandboxConfig;
import io.isolate.sdk.models.SandboxInfo;
import io.isolate.sdk.models.SandboxInfo.SandboxMetrics;
import isolate.v1.IsolateServiceGrpc;
import isolate.v1.Isolate;

import javax.net.ssl.SSLException;
import java.io.File;
import java.util.List;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.logging.Level;
import java.util.logging.Logger;
import java.util.stream.Collectors;

/**
 * Client for the Isolate gRPC sandbox service.
 *
 * <p>Provides both synchronous and asynchronous (CompletableFuture-based) methods
 * for managing sandboxes. Implements {@link AutoCloseable} for proper resource
 * management.</p>
 *
 * <h3>Basic usage:</h3>
 * <pre>{@code
 * IsolateClientConfig config = IsolateClientConfig.builder()
 *     .target("localhost:50051")
 *     .build();
 *
 * try (IsolateClient client = IsolateClient.create(config)) {
 *     // Create a sandbox
 *     SandboxConfig sandboxConfig = SandboxConfig.builder()
 *         .memoryLimit(64 * 1024 * 1024)
 *         .fuelLimit(1_000_000)
 *         .addCapability("stdout")
 *         .build();
 *
 *     String sandboxId = client.createSandbox(wasmBytes, sandboxConfig);
 *
 *     // Run the sandbox
 *     RunResult result = client.runSandbox(sandboxId, new byte[0]);
 *     System.out.println("Exit code: " + result.getExitCode());
 *     System.out.println("Output: " + result.getStdoutString());
 * }
 * }</pre>
 *
 * <h3>Async usage:</h3>
 * <pre>{@code
 * client.createSandboxAsync(wasmBytes, sandboxConfig)
 *     .thenCompose(sandboxId -> client.runSandboxAsync(sandboxId, new byte[0]))
 *     .thenAccept(result -> System.out.println("Output: " + result.getStdoutString()))
 *     .exceptionally(ex -> {
 *         System.err.println("Failed: " + ex.getMessage());
 *         return null;
 *     });
 * }</pre>
 */
public final class IsolateClient implements AutoCloseable {

    private static final Logger logger = Logger.getLogger(IsolateClient.class.getName());

    private final IsolateClientConfig config;
    private final ManagedChannel channel;
    private final IsolateServiceGrpc.IsolateServiceBlockingStub blockingStub;
    private final ExecutorService asyncExecutor;
    private final AtomicBoolean closed = new AtomicBoolean(false);

    private IsolateClient(IsolateClientConfig config, ManagedChannel channel) {
        this.config = config;
        this.channel = channel;
        this.blockingStub = IsolateServiceGrpc.newBlockingStub(channel);
        this.asyncExecutor = Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "isolate-client-async");
            t.setDaemon(true);
            return t;
        });
    }

    /**
     * Creates a new {@link IsolateClient} with the given configuration.
     *
     * <p>This establishes a gRPC channel to the configured target address. The
     * channel is lazily connected on the first RPC call.</p>
     *
     * @param config the client configuration
     * @return a new IsolateClient instance
     * @throws IsolateException if the client cannot be created (e.g., TLS setup failure)
     */
    public static IsolateClient create(IsolateClientConfig config) throws IsolateException {
        Objects.requireNonNull(config, "config must not be null");
        ManagedChannel channel = buildChannel(config);
        return new IsolateClient(config, channel);
    }

    // -----------------------------------------------------------------------
    // Synchronous API
    // -----------------------------------------------------------------------

    /**
     * Creates a new sandbox with the given WASM module and configuration.
     *
     * @param wasmModule the WASM module bytes
     * @param sandboxConfig the sandbox configuration
     * @return the ID of the created sandbox
     * @throws IsolateException if sandbox creation fails
     * @throws SandboxExecutionException if the server rejects the request
     */
    public String createSandbox(byte[] wasmModule, SandboxConfig sandboxConfig) throws IsolateException {
        ensureOpen();
        Objects.requireNonNull(wasmModule, "wasmModule must not be null");
        Objects.requireNonNull(sandboxConfig, "sandboxConfig must not be null");

        Isolate.CreateSandboxRequest request = Isolate.CreateSandboxRequest.newBuilder()
                .setModule(ByteString.copyFrom(wasmModule))
                .setConfig(toProtoConfig(sandboxConfig))
                .build();

        return executeWithRetry("createSandbox", null, () -> {
            Isolate.CreateSandboxResponse response = stubWithTimeout().createSandbox(request);
            return response.getSandboxId();
        });
    }

    /**
     * Runs a sandbox with the given input data.
     *
     * @param sandboxId the sandbox ID
     * @param input the input data to provide to the sandbox (may be empty)
     * @return the run result containing exit code, output, and resource usage
     * @throws SandboxNotFoundException if the sandbox does not exist
     * @throws SandboxExecutionException if execution fails
     * @throws IsolateException if a communication error occurs
     */
    public RunResult runSandbox(String sandboxId, byte[] input) throws IsolateException {
        return runSandbox(sandboxId, input, "_start");
    }

    /**
     * Runs a sandbox with the given input data and entry point.
     *
     * @param sandboxId the sandbox ID
     * @param input the input data to provide to the sandbox (may be empty)
     * @param entryPoint the entry point function name (default: "_start")
     * @return the run result containing exit code, output, and resource usage
     * @throws SandboxNotFoundException if the sandbox does not exist
     * @throws SandboxExecutionException if execution fails
     * @throws IsolateException if a communication error occurs
     */
    public RunResult runSandbox(String sandboxId, byte[] input, String entryPoint) throws IsolateException {
        ensureOpen();
        Objects.requireNonNull(sandboxId, "sandboxId must not be null");
        Objects.requireNonNull(input, "input must not be null");
        Objects.requireNonNull(entryPoint, "entryPoint must not be null");

        Isolate.RunSandboxRequest request = Isolate.RunSandboxRequest.newBuilder()
                .setSandboxId(sandboxId)
                .setInput(ByteString.copyFrom(input))
                .setEntryPoint(entryPoint)
                .build();

        return executeWithRetry("runSandbox", sandboxId, () -> {
            Isolate.RunSandboxResponse response = stubWithTimeout().runSandbox(request);
            return fromProtoRunResponse(response);
        });
    }

    /**
     * Retrieves information about a sandbox.
     *
     * @param sandboxId the sandbox ID
     * @return the sandbox information
     * @throws SandboxNotFoundException if the sandbox does not exist
     * @throws IsolateException if a communication error occurs
     */
    public SandboxInfo getSandbox(String sandboxId) throws IsolateException {
        ensureOpen();
        Objects.requireNonNull(sandboxId, "sandboxId must not be null");

        Isolate.GetSandboxRequest request = Isolate.GetSandboxRequest.newBuilder()
                .setSandboxId(sandboxId)
                .build();

        return executeWithRetry("getSandbox", sandboxId, () -> {
            Isolate.GetSandboxResponse response = stubWithTimeout().getSandbox(request);
            return fromProtoSandboxInfo(response.getSandbox());
        });
    }

    /**
     * Terminates a sandbox and returns whether it was successfully terminated.
     *
     * @param sandboxId the sandbox ID
     * @return true if the sandbox was terminated
     * @throws SandboxNotFoundException if the sandbox does not exist
     * @throws IsolateException if a communication error occurs
     */
    public boolean terminateSandbox(String sandboxId) throws IsolateException {
        ensureOpen();
        Objects.requireNonNull(sandboxId, "sandboxId must not be null");

        Isolate.TerminateSandboxRequest request = Isolate.TerminateSandboxRequest.newBuilder()
                .setSandboxId(sandboxId)
                .build();

        return executeWithRetry("terminateSandbox", sandboxId, () -> {
            Isolate.TerminateSandboxResponse response = stubWithTimeout().terminateSandbox(request);
            return response.getTerminated();
        });
    }

    /**
     * Lists sandboxes with optional filtering and pagination.
     *
     * @param stateFilter optional state filter (e.g., "ready", "running"), or {@code null} for all
     * @param limit maximum number of results (0 for server default)
     * @param offset pagination offset
     * @return a list of sandbox information objects
     * @throws IsolateException if a communication error occurs
     */
    public List<SandboxInfo> listSandboxes(String stateFilter, int limit, int offset) throws IsolateException {
        ensureOpen();

        Isolate.ListSandboxesRequest.Builder requestBuilder = Isolate.ListSandboxesRequest.newBuilder()
                .setLimit(limit)
                .setOffset(offset);
        if (stateFilter != null) {
            requestBuilder.setStateFilter(stateFilter);
        }

        Isolate.ListSandboxesRequest request = requestBuilder.build();

        return executeWithRetry("listSandboxes", null, () -> {
            Isolate.ListSandboxesResponse response = stubWithTimeout().listSandboxes(request);
            return response.getSandboxesList().stream()
                    .map(IsolateClient::fromProtoSandboxInfo)
                    .collect(Collectors.toList());
        });
    }

    /**
     * Lists all sandboxes without filtering.
     *
     * @return a list of all sandbox information objects
     * @throws IsolateException if a communication error occurs
     */
    public List<SandboxInfo> listSandboxes() throws IsolateException {
        return listSandboxes(null, 0, 0);
    }

    /**
     * Retrieves metrics from the Isolate service.
     *
     * @param format the metrics format ("prometheus" or "json")
     * @return the metrics data as a string
     * @throws IsolateException if a communication error occurs
     */
    public String getMetrics(String format) throws IsolateException {
        ensureOpen();
        Objects.requireNonNull(format, "format must not be null");

        Isolate.GetMetricsRequest request = Isolate.GetMetricsRequest.newBuilder()
                .setFormat(format)
                .build();

        return executeWithRetry("getMetrics", null, () -> {
            Isolate.GetMetricsResponse response = stubWithTimeout().getMetrics(request);
            return response.getData();
        });
    }

    // -----------------------------------------------------------------------
    // Asynchronous API
    // -----------------------------------------------------------------------

    /**
     * Asynchronously creates a new sandbox.
     *
     * @param wasmModule the WASM module bytes
     * @param sandboxConfig the sandbox configuration
     * @return a CompletableFuture that completes with the sandbox ID
     * @see #createSandbox(byte[], SandboxConfig)
     */
    public CompletableFuture<String> createSandboxAsync(byte[] wasmModule, SandboxConfig sandboxConfig) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return createSandbox(wasmModule, sandboxConfig);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Asynchronously runs a sandbox with the given input data.
     *
     * @param sandboxId the sandbox ID
     * @param input the input data
     * @return a CompletableFuture that completes with the run result
     * @see #runSandbox(String, byte[])
     */
    public CompletableFuture<RunResult> runSandboxAsync(String sandboxId, byte[] input) {
        return runSandboxAsync(sandboxId, input, "_start");
    }

    /**
     * Asynchronously runs a sandbox with the given input data and entry point.
     *
     * @param sandboxId the sandbox ID
     * @param input the input data
     * @param entryPoint the entry point function name
     * @return a CompletableFuture that completes with the run result
     * @see #runSandbox(String, byte[], String)
     */
    public CompletableFuture<RunResult> runSandboxAsync(String sandboxId, byte[] input, String entryPoint) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return runSandbox(sandboxId, input, entryPoint);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Asynchronously retrieves sandbox information.
     *
     * @param sandboxId the sandbox ID
     * @return a CompletableFuture that completes with the sandbox information
     * @see #getSandbox(String)
     */
    public CompletableFuture<SandboxInfo> getSandboxAsync(String sandboxId) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return getSandbox(sandboxId);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Asynchronously terminates a sandbox.
     *
     * @param sandboxId the sandbox ID
     * @return a CompletableFuture that completes with true if the sandbox was terminated
     * @see #terminateSandbox(String)
     */
    public CompletableFuture<Boolean> terminateSandboxAsync(String sandboxId) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return terminateSandbox(sandboxId);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Asynchronously lists sandboxes.
     *
     * @param stateFilter optional state filter, or {@code null}
     * @param limit maximum number of results
     * @param offset pagination offset
     * @return a CompletableFuture that completes with the sandbox list
     * @see #listSandboxes(String, int, int)
     */
    public CompletableFuture<List<SandboxInfo>> listSandboxesAsync(String stateFilter, int limit, int offset) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return listSandboxes(stateFilter, limit, offset);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Asynchronously lists all sandboxes.
     *
     * @return a CompletableFuture that completes with the sandbox list
     * @see #listSandboxes()
     */
    public CompletableFuture<List<SandboxInfo>> listSandboxesAsync() {
        return listSandboxesAsync(null, 0, 0);
    }

    /**
     * Asynchronously retrieves metrics.
     *
     * @param format the metrics format ("prometheus" or "json")
     * @return a CompletableFuture that completes with the metrics string
     * @see #getMetrics(String)
     */
    public CompletableFuture<String> getMetricsAsync(String format) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return getMetrics(format);
            } catch (IsolateException e) {
                throw new java.util.concurrent.CompletionException(e);
            }
        }, asyncExecutor);
    }

    // -----------------------------------------------------------------------
    // AutoCloseable
    // -----------------------------------------------------------------------

    /**
     * Shuts down the client and releases all resources.
     *
     * <p>Initiates an orderly shutdown of the gRPC channel and the async executor.
     * Waits up to 5 seconds for in-flight operations to complete before forcing
     * shutdown.</p>
     */
    @Override
    public void close() {
        if (closed.compareAndSet(false, true)) {
            logger.fine("Shutting down IsolateClient");
            asyncExecutor.shutdown();
            channel.shutdown();
            try {
                if (!channel.awaitTermination(5, TimeUnit.SECONDS)) {
                    logger.warning("Channel did not terminate in time, forcing shutdown");
                    channel.shutdownNow();
                    channel.awaitTermination(2, TimeUnit.SECONDS);
                }
                if (!asyncExecutor.awaitTermination(5, TimeUnit.SECONDS)) {
                    asyncExecutor.shutdownNow();
                }
            } catch (InterruptedException e) {
                channel.shutdownNow();
                asyncExecutor.shutdownNow();
                Thread.currentThread().interrupt();
            }
            logger.fine("IsolateClient shut down successfully");
        }
    }

    /**
     * Returns whether this client has been closed.
     *
     * @return true if the client is closed
     */
    public boolean isClosed() {
        return closed.get();
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    private void ensureOpen() throws IsolateException {
        if (closed.get()) {
            throw new IsolateException("Client is closed", "ensureOpen", null, null);
        }
    }

    private IsolateServiceGrpc.IsolateServiceBlockingStub stubWithTimeout() {
        return blockingStub.withDeadlineAfter(config.getTimeout().toMillis(), TimeUnit.MILLISECONDS);
    }

    /**
     * Executes a gRPC operation with retry logic for transient failures.
     */
    private <T> T executeWithRetry(String operation, String sandboxId, GrpcOperation<T> grpcOp)
            throws IsolateException {
        int attempt = 0;
        long backoffMs = config.getRetryBackoff().toMillis();

        while (true) {
            try {
                return grpcOp.execute();
            } catch (StatusRuntimeException e) {
                attempt++;
                if (!isRetryable(e.getStatus().getCode()) || attempt > config.getMaxRetries()) {
                    throw mapException(operation, sandboxId, e);
                }
                logger.log(Level.FINE, "Retrying {0} (attempt {1}/{2}) after {3}: {4}",
                        new Object[]{operation, attempt, config.getMaxRetries(),
                                e.getStatus().getCode(), e.getMessage()});
                try {
                    Thread.sleep(backoffMs);
                } catch (InterruptedException ie) {
                    Thread.currentThread().interrupt();
                    throw new IsolateException(
                            "Interrupted during retry backoff",
                            operation,
                            null,
                            ie
                    );
                }
                // Exponential backoff with cap at 10 seconds
                backoffMs = Math.min(backoffMs * 2, 10_000);
            }
        }
    }

    private static boolean isRetryable(Status.Code code) {
        switch (code) {
            case UNAVAILABLE:
            case DEADLINE_EXCEEDED:
            case ABORTED:
                return true;
            default:
                return false;
        }
    }

    private static IsolateException mapException(String operation, String sandboxId, StatusRuntimeException e) {
        Status.Code code = e.getStatus().getCode();
        String description = e.getStatus().getDescription();
        String message = description != null ? description : e.getMessage();

        switch (code) {
            case NOT_FOUND:
                return new SandboxNotFoundException(
                        sandboxId != null ? sandboxId : "unknown",
                        operation,
                        e
                );
            case RESOURCE_EXHAUSTED:
            case FAILED_PRECONDITION:
                return new SandboxExecutionException(
                        message,
                        sandboxId,
                        null,
                        operation,
                        code,
                        e
                );
            case PERMISSION_DENIED:
                return new SandboxExecutionException(
                        "Permission denied: " + message,
                        sandboxId,
                        null,
                        operation,
                        code,
                        e
                );
            default:
                return new IsolateException(message, operation, code, e);
        }
    }

    private static ManagedChannel buildChannel(IsolateClientConfig config) throws IsolateException {
        if (config.isTlsEnabled()) {
            return buildTlsChannel(config);
        }
        ManagedChannelBuilder<?> builder = ManagedChannelBuilder.forTarget(config.getTarget())
                .usePlaintext()
                .maxInboundMessageSize(config.getMaxMessageSize());

        if (config.isKeepAliveEnabled()) {
            builder.keepAliveTime(config.getKeepAliveTime().toMillis(), TimeUnit.MILLISECONDS)
                    .keepAliveTimeout(config.getKeepAliveTimeout().toMillis(), TimeUnit.MILLISECONDS)
                    .keepAliveWithoutCalls(true);
        }

        return builder.build();
    }

    private static ManagedChannel buildTlsChannel(IsolateClientConfig config) throws IsolateException {
        try {
            SslContextBuilder sslBuilder = GrpcSslContexts.forClient();

            if (config.getTlsCaCertPath() != null) {
                sslBuilder.trustManager(new File(config.getTlsCaCertPath()));
            }
            if (config.getTlsCertPath() != null && config.getTlsKeyPath() != null) {
                sslBuilder.keyManager(
                        new File(config.getTlsCertPath()),
                        new File(config.getTlsKeyPath())
                );
            }

            SslContext sslContext = sslBuilder.build();

            NettyChannelBuilder builder = NettyChannelBuilder.forTarget(config.getTarget())
                    .sslContext(sslContext)
                    .maxInboundMessageSize(config.getMaxMessageSize());

            if (config.isKeepAliveEnabled()) {
                builder.keepAliveTime(config.getKeepAliveTime().toMillis(), TimeUnit.MILLISECONDS)
                        .keepAliveTimeout(config.getKeepAliveTimeout().toMillis(), TimeUnit.MILLISECONDS)
                        .keepAliveWithoutCalls(true);
            }

            return builder.build();
        } catch (SSLException e) {
            throw new IsolateException("Failed to configure TLS", "connect", null, e);
        }
    }

    // -----------------------------------------------------------------------
    // Proto conversion helpers
    // -----------------------------------------------------------------------

    private static Isolate.SandboxConfig toProtoConfig(SandboxConfig config) {
        Isolate.SandboxConfig.Builder builder = Isolate.SandboxConfig.newBuilder()
                .setMemoryLimit(config.getMemoryLimit())
                .setFuelLimit(config.getFuelLimit())
                .setWallTimeLimitSecs(config.getWallTimeLimitSecs())
                .setCpuTimeLimitSecs(config.getCpuTimeLimitSecs());

        for (SandboxConfig.Capability cap : config.getCapabilities()) {
            builder.addCapabilities(Isolate.Capability.newBuilder()
                    .setType(cap.getType())
                    .setValue(cap.getValue())
                    .build());
        }

        builder.putAllEnv(config.getEnv());
        builder.addAllArgs(config.getArgs());

        return builder.build();
    }

    private static RunResult fromProtoRunResponse(Isolate.RunSandboxResponse response) {
        RunResult.Builder builder = RunResult.builder()
                .exitCode(response.getExitCode())
                .stdout(response.getStdout().toByteArray())
                .stderr(response.getStderr().toByteArray())
                .durationMs(response.getDurationMs());

        if (response.hasResourceUsage()) {
            builder.resourceUsage(fromProtoResourceUsage(response.getResourceUsage()));
        }

        return builder.build();
    }

    private static ResourceUsage fromProtoResourceUsage(Isolate.ResourceUsage proto) {
        return ResourceUsage.builder()
                .peakMemory(proto.getPeakMemory())
                .fuelConsumed(proto.getFuelConsumed())
                .cpuTimeMs(proto.getCpuTimeMs())
                .wallTimeMs(proto.getWallTimeMs())
                .bytesRead(proto.getBytesRead())
                .bytesWritten(proto.getBytesWritten())
                .build();
    }

    private static SandboxInfo fromProtoSandboxInfo(Isolate.SandboxInfo proto) {
        SandboxInfo.Builder builder = SandboxInfo.builder()
                .id(proto.getId())
                .state(proto.getState())
                .moduleHash(proto.getModuleHash())
                .createdAt(proto.getCreatedAt())
                .ageSecs(proto.getAgeSecs());

        if (proto.hasMetrics()) {
            Isolate.SandboxMetrics m = proto.getMetrics();
            builder.metrics(new SandboxMetrics(
                    m.getRunCount(),
                    m.getSuccessCount(),
                    m.getFailureCount(),
                    m.getTotalRunDurationMs(),
                    m.getLastRunDurationMs()
            ));
        }

        return builder.build();
    }

    /**
     * Functional interface for gRPC operations that can throw StatusRuntimeException.
     */
    @FunctionalInterface
    private interface GrpcOperation<T> {
        T execute() throws StatusRuntimeException;
    }
}
