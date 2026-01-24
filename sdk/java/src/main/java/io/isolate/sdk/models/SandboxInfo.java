package io.isolate.sdk.models;

import java.util.Objects;
import java.util.Optional;

/**
 * Information about a sandbox, including its current state and metrics.
 *
 * <p>Instances are immutable and typically returned by
 * {@code IsolateClient.getSandbox()} and {@code IsolateClient.listSandboxes()}.</p>
 */
public final class SandboxInfo {

    private final String id;
    private final String state;
    private final String moduleHash;
    private final long createdAt;
    private final double ageSecs;
    private final SandboxMetrics metrics;

    private SandboxInfo(Builder builder) {
        this.id = builder.id;
        this.state = builder.state;
        this.moduleHash = builder.moduleHash;
        this.createdAt = builder.createdAt;
        this.ageSecs = builder.ageSecs;
        this.metrics = builder.metrics;
    }

    /**
     * Returns a new builder for {@link SandboxInfo}.
     *
     * @return a new builder instance
     */
    public static Builder builder() {
        return new Builder();
    }

    /**
     * Returns the sandbox ID.
     *
     * @return the sandbox ID
     */
    public String getId() {
        return id;
    }

    /**
     * Returns the current state of the sandbox (e.g., "ready", "running", "terminated").
     *
     * @return the sandbox state
     */
    public String getState() {
        return state;
    }

    /**
     * Returns the SHA-256 hash of the WASM module.
     *
     * @return the module hash
     */
    public String getModuleHash() {
        return moduleHash;
    }

    /**
     * Returns the creation timestamp as a Unix epoch in seconds.
     *
     * @return the creation timestamp
     */
    public long getCreatedAt() {
        return createdAt;
    }

    /**
     * Returns the age of the sandbox in seconds.
     *
     * @return age in seconds
     */
    public double getAgeSecs() {
        return ageSecs;
    }

    /**
     * Returns the sandbox execution metrics, if available.
     *
     * @return an Optional containing the metrics, or empty if not available
     */
    public Optional<SandboxMetrics> getMetrics() {
        return Optional.ofNullable(metrics);
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (o == null || getClass() != o.getClass()) return false;
        SandboxInfo that = (SandboxInfo) o;
        return createdAt == that.createdAt
                && Double.compare(that.ageSecs, ageSecs) == 0
                && Objects.equals(id, that.id)
                && Objects.equals(state, that.state)
                && Objects.equals(moduleHash, that.moduleHash)
                && Objects.equals(metrics, that.metrics);
    }

    @Override
    public int hashCode() {
        return Objects.hash(id, state, moduleHash, createdAt, ageSecs, metrics);
    }

    @Override
    public String toString() {
        return "SandboxInfo{"
                + "id='" + id + '\''
                + ", state='" + state + '\''
                + ", moduleHash='" + moduleHash + '\''
                + ", createdAt=" + createdAt
                + ", ageSecs=" + ageSecs
                + ", metrics=" + metrics
                + '}';
    }

    /**
     * Execution metrics for a sandbox.
     */
    public static final class SandboxMetrics {

        private final long runCount;
        private final long successCount;
        private final long failureCount;
        private final double totalRunDurationMs;
        private final double lastRunDurationMs;

        /**
         * Creates sandbox metrics.
         *
         * @param runCount            total number of runs
         * @param successCount        number of successful runs
         * @param failureCount        number of failed runs
         * @param totalRunDurationMs  total run duration in milliseconds
         * @param lastRunDurationMs   last run duration in milliseconds
         */
        public SandboxMetrics(long runCount, long successCount, long failureCount,
                              double totalRunDurationMs, double lastRunDurationMs) {
            this.runCount = runCount;
            this.successCount = successCount;
            this.failureCount = failureCount;
            this.totalRunDurationMs = totalRunDurationMs;
            this.lastRunDurationMs = lastRunDurationMs;
        }

        /** Returns the total number of runs. */
        public long getRunCount() {
            return runCount;
        }

        /** Returns the number of successful runs. */
        public long getSuccessCount() {
            return successCount;
        }

        /** Returns the number of failed runs. */
        public long getFailureCount() {
            return failureCount;
        }

        /** Returns the total run duration in milliseconds. */
        public double getTotalRunDurationMs() {
            return totalRunDurationMs;
        }

        /** Returns the last run duration in milliseconds. */
        public double getLastRunDurationMs() {
            return lastRunDurationMs;
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            if (o == null || getClass() != o.getClass()) return false;
            SandboxMetrics that = (SandboxMetrics) o;
            return runCount == that.runCount
                    && successCount == that.successCount
                    && failureCount == that.failureCount
                    && Double.compare(that.totalRunDurationMs, totalRunDurationMs) == 0
                    && Double.compare(that.lastRunDurationMs, lastRunDurationMs) == 0;
        }

        @Override
        public int hashCode() {
            return Objects.hash(runCount, successCount, failureCount,
                    totalRunDurationMs, lastRunDurationMs);
        }

        @Override
        public String toString() {
            return "SandboxMetrics{"
                    + "runCount=" + runCount
                    + ", successCount=" + successCount
                    + ", failureCount=" + failureCount
                    + ", totalRunDurationMs=" + totalRunDurationMs
                    + ", lastRunDurationMs=" + lastRunDurationMs
                    + '}';
        }
    }

    /**
     * Builder for {@link SandboxInfo}.
     */
    public static final class Builder {

        private String id;
        private String state;
        private String moduleHash;
        private long createdAt;
        private double ageSecs;
        private SandboxMetrics metrics;

        private Builder() {
        }

        /**
         * Sets the sandbox ID.
         *
         * @param id the sandbox ID
         * @return this builder
         */
        public Builder id(String id) {
            this.id = id;
            return this;
        }

        /**
         * Sets the sandbox state.
         *
         * @param state the state
         * @return this builder
         */
        public Builder state(String state) {
            this.state = state;
            return this;
        }

        /**
         * Sets the module hash.
         *
         * @param moduleHash the module hash
         * @return this builder
         */
        public Builder moduleHash(String moduleHash) {
            this.moduleHash = moduleHash;
            return this;
        }

        /**
         * Sets the creation timestamp.
         *
         * @param createdAt the creation timestamp (Unix epoch seconds)
         * @return this builder
         */
        public Builder createdAt(long createdAt) {
            this.createdAt = createdAt;
            return this;
        }

        /**
         * Sets the age in seconds.
         *
         * @param ageSecs age in seconds
         * @return this builder
         */
        public Builder ageSecs(double ageSecs) {
            this.ageSecs = ageSecs;
            return this;
        }

        /**
         * Sets the sandbox metrics.
         *
         * @param metrics the metrics, or {@code null}
         * @return this builder
         */
        public Builder metrics(SandboxMetrics metrics) {
            this.metrics = metrics;
            return this;
        }

        /**
         * Builds the {@link SandboxInfo} instance.
         *
         * @return a new immutable SandboxInfo
         */
        public SandboxInfo build() {
            return new SandboxInfo(this);
        }
    }
}
