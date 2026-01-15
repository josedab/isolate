package io.isolate.sdk.models;

import java.util.Objects;

/**
 * Resource usage statistics from a sandbox execution.
 *
 * <p>Instances of this class are immutable and contain the resource consumption
 * metrics collected during a sandbox run, including memory, CPU, fuel, and I/O.</p>
 */
public final class ResourceUsage {

    private final long peakMemory;
    private final long fuelConsumed;
    private final double cpuTimeMs;
    private final double wallTimeMs;
    private final long bytesRead;
    private final long bytesWritten;

    private ResourceUsage(Builder builder) {
        this.peakMemory = builder.peakMemory;
        this.fuelConsumed = builder.fuelConsumed;
        this.cpuTimeMs = builder.cpuTimeMs;
        this.wallTimeMs = builder.wallTimeMs;
        this.bytesRead = builder.bytesRead;
        this.bytesWritten = builder.bytesWritten;
    }

    /**
     * Returns a new builder for {@link ResourceUsage}.
     *
     * @return a new builder instance
     */
    public static Builder builder() {
        return new Builder();
    }

    /**
     * Returns the peak memory usage in bytes.
     *
     * @return peak memory in bytes
     */
    public long getPeakMemory() {
        return peakMemory;
    }

    /**
     * Returns the total fuel consumed during execution.
     *
     * @return fuel consumed
     */
    public long getFuelConsumed() {
        return fuelConsumed;
    }

    /**
     * Returns the CPU time consumed in milliseconds.
     *
     * @return CPU time in milliseconds
     */
    public double getCpuTimeMs() {
        return cpuTimeMs;
    }

    /**
     * Returns the wall-clock time consumed in milliseconds.
     *
     * @return wall time in milliseconds
     */
    public double getWallTimeMs() {
        return wallTimeMs;
    }

    /**
     * Returns the total bytes read during execution.
     *
     * @return bytes read
     */
    public long getBytesRead() {
        return bytesRead;
    }

    /**
     * Returns the total bytes written during execution.
     *
     * @return bytes written
     */
    public long getBytesWritten() {
        return bytesWritten;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (o == null || getClass() != o.getClass()) return false;
        ResourceUsage that = (ResourceUsage) o;
        return peakMemory == that.peakMemory
                && fuelConsumed == that.fuelConsumed
                && Double.compare(that.cpuTimeMs, cpuTimeMs) == 0
                && Double.compare(that.wallTimeMs, wallTimeMs) == 0
                && bytesRead == that.bytesRead
                && bytesWritten == that.bytesWritten;
    }

    @Override
    public int hashCode() {
        return Objects.hash(peakMemory, fuelConsumed, cpuTimeMs, wallTimeMs, bytesRead, bytesWritten);
    }

    @Override
    public String toString() {
        return "ResourceUsage{"
                + "peakMemory=" + peakMemory
                + ", fuelConsumed=" + fuelConsumed
                + ", cpuTimeMs=" + cpuTimeMs
                + ", wallTimeMs=" + wallTimeMs
                + ", bytesRead=" + bytesRead
                + ", bytesWritten=" + bytesWritten
                + '}';
    }

    /**
     * Builder for {@link ResourceUsage}.
     */
    public static final class Builder {

        private long peakMemory;
        private long fuelConsumed;
        private double cpuTimeMs;
        private double wallTimeMs;
        private long bytesRead;
        private long bytesWritten;

        private Builder() {
        }

        /**
         * Sets the peak memory usage in bytes.
         *
         * @param peakMemory peak memory in bytes
         * @return this builder
         */
        public Builder peakMemory(long peakMemory) {
            this.peakMemory = peakMemory;
            return this;
        }

        /**
         * Sets the fuel consumed.
         *
         * @param fuelConsumed fuel consumed
         * @return this builder
         */
        public Builder fuelConsumed(long fuelConsumed) {
            this.fuelConsumed = fuelConsumed;
            return this;
        }

        /**
         * Sets the CPU time in milliseconds.
         *
         * @param cpuTimeMs CPU time in milliseconds
         * @return this builder
         */
        public Builder cpuTimeMs(double cpuTimeMs) {
            this.cpuTimeMs = cpuTimeMs;
            return this;
        }

        /**
         * Sets the wall-clock time in milliseconds.
         *
         * @param wallTimeMs wall time in milliseconds
         * @return this builder
         */
        public Builder wallTimeMs(double wallTimeMs) {
            this.wallTimeMs = wallTimeMs;
            return this;
        }

        /**
         * Sets the bytes read.
         *
         * @param bytesRead bytes read
         * @return this builder
         */
        public Builder bytesRead(long bytesRead) {
            this.bytesRead = bytesRead;
            return this;
        }

        /**
         * Sets the bytes written.
         *
         * @param bytesWritten bytes written
         * @return this builder
         */
        public Builder bytesWritten(long bytesWritten) {
            this.bytesWritten = bytesWritten;
            return this;
        }

        /**
         * Builds the {@link ResourceUsage} instance.
         *
         * @return a new immutable ResourceUsage
         */
        public ResourceUsage build() {
            return new ResourceUsage(this);
        }
    }
}
