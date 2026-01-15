package io.isolate.sdk;

import java.io.File;
import java.time.Duration;
import java.util.Objects;

/**
 * Configuration for the {@link IsolateClient}.
 *
 * <p>Use the {@link #builder()} method to construct instances. At minimum, a
 * target address must be provided.</p>
 *
 * <pre>{@code
 * IsolateClientConfig config = IsolateClientConfig.builder()
 *     .target("localhost:50051")
 *     .timeout(Duration.ofSeconds(30))
 *     .maxRetries(3)
 *     .build();
 * }</pre>
 *
 * <p>For TLS connections:</p>
 * <pre>{@code
 * IsolateClientConfig config = IsolateClientConfig.builder()
 *     .target("isolate.example.com:443")
 *     .tlsEnabled(true)
 *     .tlsCertPath("/path/to/ca.pem")
 *     .build();
 * }</pre>
 */
public final class IsolateClientConfig {

    /** Default timeout for individual RPC calls. */
    public static final Duration DEFAULT_TIMEOUT = Duration.ofSeconds(30);

    /** Default maximum number of retries for transient failures. */
    public static final int DEFAULT_MAX_RETRIES = 3;

    /** Default initial backoff duration for retry delays. */
    public static final Duration DEFAULT_RETRY_BACKOFF = Duration.ofMillis(100);

    /** Default maximum message size (16 MB). */
    public static final int DEFAULT_MAX_MESSAGE_SIZE = 16 * 1024 * 1024;

    private final String target;
    private final Duration timeout;
    private final int maxRetries;
    private final Duration retryBackoff;
    private final boolean tlsEnabled;
    private final String tlsCertPath;
    private final String tlsKeyPath;
    private final String tlsCaCertPath;
    private final int maxMessageSize;
    private final boolean keepAliveEnabled;
    private final Duration keepAliveTime;
    private final Duration keepAliveTimeout;

    private IsolateClientConfig(Builder builder) {
        this.target = builder.target;
        this.timeout = builder.timeout;
        this.maxRetries = builder.maxRetries;
        this.retryBackoff = builder.retryBackoff;
        this.tlsEnabled = builder.tlsEnabled;
        this.tlsCertPath = builder.tlsCertPath;
        this.tlsKeyPath = builder.tlsKeyPath;
        this.tlsCaCertPath = builder.tlsCaCertPath;
        this.maxMessageSize = builder.maxMessageSize;
        this.keepAliveEnabled = builder.keepAliveEnabled;
        this.keepAliveTime = builder.keepAliveTime;
        this.keepAliveTimeout = builder.keepAliveTimeout;
    }

    /**
     * Returns a new builder for {@link IsolateClientConfig}.
     *
     * @return a new builder instance
     */
    public static Builder builder() {
        return new Builder();
    }

    /**
     * Returns the gRPC target address (e.g., "localhost:50051").
     *
     * @return the target address
     */
    public String getTarget() {
        return target;
    }

    /**
     * Returns the default timeout for RPC calls.
     *
     * @return the timeout duration
     */
    public Duration getTimeout() {
        return timeout;
    }

    /**
     * Returns the maximum number of retries for transient failures.
     *
     * @return the max retry count
     */
    public int getMaxRetries() {
        return maxRetries;
    }

    /**
     * Returns the initial backoff duration between retries.
     *
     * @return the retry backoff duration
     */
    public Duration getRetryBackoff() {
        return retryBackoff;
    }

    /**
     * Returns whether TLS is enabled for the connection.
     *
     * @return true if TLS is enabled
     */
    public boolean isTlsEnabled() {
        return tlsEnabled;
    }

    /**
     * Returns the path to the TLS client certificate, or {@code null} if not set.
     *
     * @return the TLS certificate path
     */
    public String getTlsCertPath() {
        return tlsCertPath;
    }

    /**
     * Returns the path to the TLS client private key, or {@code null} if not set.
     *
     * @return the TLS key path
     */
    public String getTlsKeyPath() {
        return tlsKeyPath;
    }

    /**
     * Returns the path to the CA certificate for server verification, or {@code null} if not set.
     *
     * @return the CA certificate path
     */
    public String getTlsCaCertPath() {
        return tlsCaCertPath;
    }

    /**
     * Returns the maximum message size in bytes.
     *
     * @return the max message size
     */
    public int getMaxMessageSize() {
        return maxMessageSize;
    }

    /**
     * Returns whether keep-alive is enabled.
     *
     * @return true if keep-alive is enabled
     */
    public boolean isKeepAliveEnabled() {
        return keepAliveEnabled;
    }

    /**
     * Returns the keep-alive ping interval.
     *
     * @return the keep-alive time
     */
    public Duration getKeepAliveTime() {
        return keepAliveTime;
    }

    /**
     * Returns the keep-alive timeout.
     *
     * @return the keep-alive timeout
     */
    public Duration getKeepAliveTimeout() {
        return keepAliveTimeout;
    }

    @Override
    public String toString() {
        return "IsolateClientConfig{"
                + "target='" + target + '\''
                + ", timeout=" + timeout
                + ", maxRetries=" + maxRetries
                + ", tlsEnabled=" + tlsEnabled
                + ", maxMessageSize=" + maxMessageSize
                + ", keepAliveEnabled=" + keepAliveEnabled
                + '}';
    }

    /**
     * Builder for {@link IsolateClientConfig}.
     */
    public static final class Builder {

        private String target;
        private Duration timeout = DEFAULT_TIMEOUT;
        private int maxRetries = DEFAULT_MAX_RETRIES;
        private Duration retryBackoff = DEFAULT_RETRY_BACKOFF;
        private boolean tlsEnabled = false;
        private String tlsCertPath;
        private String tlsKeyPath;
        private String tlsCaCertPath;
        private int maxMessageSize = DEFAULT_MAX_MESSAGE_SIZE;
        private boolean keepAliveEnabled = false;
        private Duration keepAliveTime = Duration.ofSeconds(30);
        private Duration keepAliveTimeout = Duration.ofSeconds(10);

        private Builder() {
        }

        /**
         * Sets the gRPC target address.
         *
         * <p>This is required. The address should include host and port, e.g.,
         * "localhost:50051" or "isolate.example.com:443".</p>
         *
         * @param target the target address
         * @return this builder
         */
        public Builder target(String target) {
            this.target = Objects.requireNonNull(target, "target must not be null");
            return this;
        }

        /**
         * Sets the default timeout for RPC calls.
         *
         * @param timeout the timeout duration (must be positive)
         * @return this builder
         * @throws IllegalArgumentException if timeout is null or non-positive
         */
        public Builder timeout(Duration timeout) {
            Objects.requireNonNull(timeout, "timeout must not be null");
            if (timeout.isNegative() || timeout.isZero()) {
                throw new IllegalArgumentException("timeout must be positive, got: " + timeout);
            }
            this.timeout = timeout;
            return this;
        }

        /**
         * Sets the maximum number of retries for transient failures.
         *
         * @param maxRetries the maximum retry count (must be non-negative)
         * @return this builder
         * @throws IllegalArgumentException if maxRetries is negative
         */
        public Builder maxRetries(int maxRetries) {
            if (maxRetries < 0) {
                throw new IllegalArgumentException("maxRetries must be non-negative, got: " + maxRetries);
            }
            this.maxRetries = maxRetries;
            return this;
        }

        /**
         * Sets the initial backoff duration between retries.
         *
         * <p>Subsequent retries use exponential backoff based on this value.</p>
         *
         * @param retryBackoff the initial backoff duration
         * @return this builder
         */
        public Builder retryBackoff(Duration retryBackoff) {
            this.retryBackoff = Objects.requireNonNull(retryBackoff, "retryBackoff must not be null");
            return this;
        }

        /**
         * Enables or disables TLS for the gRPC connection.
         *
         * @param tlsEnabled true to enable TLS
         * @return this builder
         */
        public Builder tlsEnabled(boolean tlsEnabled) {
            this.tlsEnabled = tlsEnabled;
            return this;
        }

        /**
         * Sets the path to the TLS client certificate file.
         *
         * <p>Used for mutual TLS (mTLS) authentication.</p>
         *
         * @param tlsCertPath path to the PEM-encoded certificate
         * @return this builder
         */
        public Builder tlsCertPath(String tlsCertPath) {
            this.tlsCertPath = tlsCertPath;
            return this;
        }

        /**
         * Sets the path to the TLS client private key file.
         *
         * <p>Used for mutual TLS (mTLS) authentication.</p>
         *
         * @param tlsKeyPath path to the PEM-encoded private key
         * @return this builder
         */
        public Builder tlsKeyPath(String tlsKeyPath) {
            this.tlsKeyPath = tlsKeyPath;
            return this;
        }

        /**
         * Sets the path to the CA certificate for verifying the server.
         *
         * @param tlsCaCertPath path to the PEM-encoded CA certificate
         * @return this builder
         */
        public Builder tlsCaCertPath(String tlsCaCertPath) {
            this.tlsCaCertPath = tlsCaCertPath;
            return this;
        }

        /**
         * Sets the maximum message size in bytes.
         *
         * @param maxMessageSize the maximum message size (must be positive)
         * @return this builder
         * @throws IllegalArgumentException if maxMessageSize is not positive
         */
        public Builder maxMessageSize(int maxMessageSize) {
            if (maxMessageSize <= 0) {
                throw new IllegalArgumentException("maxMessageSize must be positive, got: " + maxMessageSize);
            }
            this.maxMessageSize = maxMessageSize;
            return this;
        }

        /**
         * Enables or disables gRPC keep-alive pings.
         *
         * @param keepAliveEnabled true to enable keep-alive
         * @return this builder
         */
        public Builder keepAliveEnabled(boolean keepAliveEnabled) {
            this.keepAliveEnabled = keepAliveEnabled;
            return this;
        }

        /**
         * Sets the interval between keep-alive pings.
         *
         * @param keepAliveTime the keep-alive interval
         * @return this builder
         */
        public Builder keepAliveTime(Duration keepAliveTime) {
            this.keepAliveTime = Objects.requireNonNull(keepAliveTime, "keepAliveTime must not be null");
            return this;
        }

        /**
         * Sets the timeout for keep-alive ping responses.
         *
         * @param keepAliveTimeout the keep-alive timeout
         * @return this builder
         */
        public Builder keepAliveTimeout(Duration keepAliveTimeout) {
            this.keepAliveTimeout = Objects.requireNonNull(keepAliveTimeout, "keepAliveTimeout must not be null");
            return this;
        }

        /**
         * Builds the {@link IsolateClientConfig} instance.
         *
         * @return a new immutable IsolateClientConfig
         * @throws IllegalStateException if required fields are missing
         */
        public IsolateClientConfig build() {
            if (target == null || target.isEmpty()) {
                throw new IllegalStateException("target must be set (e.g., \"localhost:50051\")");
            }
            if (tlsCertPath != null && !new File(tlsCertPath).exists()) {
                throw new IllegalStateException("TLS certificate file does not exist: " + tlsCertPath);
            }
            if (tlsKeyPath != null && !new File(tlsKeyPath).exists()) {
                throw new IllegalStateException("TLS key file does not exist: " + tlsKeyPath);
            }
            if (tlsCaCertPath != null && !new File(tlsCaCertPath).exists()) {
                throw new IllegalStateException("TLS CA certificate file does not exist: " + tlsCaCertPath);
            }
            return new IsolateClientConfig(this);
        }
    }
}
