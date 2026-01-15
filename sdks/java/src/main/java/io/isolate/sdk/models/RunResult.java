package io.isolate.sdk.models;

import java.util.Objects;
import java.util.Optional;

/**
 * The result of a sandbox execution.
 *
 * <p>Contains the exit code, captured stdout/stderr output, execution duration,
 * and resource usage metrics. Instances are immutable.</p>
 *
 * <pre>{@code
 * RunResult result = client.runSandbox(sandboxId, input);
 * System.out.println("Exit code: " + result.getExitCode());
 * System.out.println("Output: " + new String(result.getStdout()));
 * result.getResourceUsage().ifPresent(usage ->
 *     System.out.println("Memory used: " + usage.getPeakMemory()));
 * }</pre>
 */
public final class RunResult {

    private final int exitCode;
    private final byte[] stdout;
    private final byte[] stderr;
    private final double durationMs;
    private final ResourceUsage resourceUsage;

    private RunResult(Builder builder) {
        this.exitCode = builder.exitCode;
        this.stdout = builder.stdout != null ? builder.stdout.clone() : new byte[0];
        this.stderr = builder.stderr != null ? builder.stderr.clone() : new byte[0];
        this.durationMs = builder.durationMs;
        this.resourceUsage = builder.resourceUsage;
    }

    /**
     * Returns a new builder for {@link RunResult}.
     *
     * @return a new builder instance
     */
    public static Builder builder() {
        return new Builder();
    }

    /**
     * Returns the exit code from the sandbox execution.
     *
     * <p>A value of 0 typically indicates success.</p>
     *
     * @return the exit code
     */
    public int getExitCode() {
        return exitCode;
    }

    /**
     * Returns a copy of the captured stdout bytes.
     *
     * @return stdout output as a byte array (never null)
     */
    public byte[] getStdout() {
        return stdout.clone();
    }

    /**
     * Returns the captured stdout as a UTF-8 string.
     *
     * @return stdout output as a string
     */
    public String getStdoutString() {
        return new String(stdout, java.nio.charset.StandardCharsets.UTF_8);
    }

    /**
     * Returns a copy of the captured stderr bytes.
     *
     * @return stderr output as a byte array (never null)
     */
    public byte[] getStderr() {
        return stderr.clone();
    }

    /**
     * Returns the captured stderr as a UTF-8 string.
     *
     * @return stderr output as a string
     */
    public String getStderrString() {
        return new String(stderr, java.nio.charset.StandardCharsets.UTF_8);
    }

    /**
     * Returns the execution duration in milliseconds.
     *
     * @return duration in milliseconds
     */
    public double getDurationMs() {
        return durationMs;
    }

    /**
     * Returns the resource usage from the execution, if available.
     *
     * @return an Optional containing the resource usage, or empty
     */
    public Optional<ResourceUsage> getResourceUsage() {
        return Optional.ofNullable(resourceUsage);
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (o == null || getClass() != o.getClass()) return false;
        RunResult that = (RunResult) o;
        return exitCode == that.exitCode
                && Double.compare(that.durationMs, durationMs) == 0
                && java.util.Arrays.equals(stdout, that.stdout)
                && java.util.Arrays.equals(stderr, that.stderr)
                && Objects.equals(resourceUsage, that.resourceUsage);
    }

    @Override
    public int hashCode() {
        int result = Objects.hash(exitCode, durationMs, resourceUsage);
        result = 31 * result + java.util.Arrays.hashCode(stdout);
        result = 31 * result + java.util.Arrays.hashCode(stderr);
        return result;
    }

    @Override
    public String toString() {
        return "RunResult{"
                + "exitCode=" + exitCode
                + ", stdoutLength=" + stdout.length
                + ", stderrLength=" + stderr.length
                + ", durationMs=" + durationMs
                + ", resourceUsage=" + resourceUsage
                + '}';
    }

    /**
     * Builder for {@link RunResult}.
     */
    public static final class Builder {

        private int exitCode;
        private byte[] stdout;
        private byte[] stderr;
        private double durationMs;
        private ResourceUsage resourceUsage;

        private Builder() {
        }

        /**
         * Sets the exit code.
         *
         * @param exitCode the exit code
         * @return this builder
         */
        public Builder exitCode(int exitCode) {
            this.exitCode = exitCode;
            return this;
        }

        /**
         * Sets the stdout output.
         *
         * @param stdout stdout bytes
         * @return this builder
         */
        public Builder stdout(byte[] stdout) {
            this.stdout = stdout;
            return this;
        }

        /**
         * Sets the stderr output.
         *
         * @param stderr stderr bytes
         * @return this builder
         */
        public Builder stderr(byte[] stderr) {
            this.stderr = stderr;
            return this;
        }

        /**
         * Sets the execution duration in milliseconds.
         *
         * @param durationMs duration in milliseconds
         * @return this builder
         */
        public Builder durationMs(double durationMs) {
            this.durationMs = durationMs;
            return this;
        }

        /**
         * Sets the resource usage.
         *
         * @param resourceUsage the resource usage, or {@code null}
         * @return this builder
         */
        public Builder resourceUsage(ResourceUsage resourceUsage) {
            this.resourceUsage = resourceUsage;
            return this;
        }

        /**
         * Builds the {@link RunResult} instance.
         *
         * @return a new immutable RunResult
         */
        public RunResult build() {
            return new RunResult(this);
        }
    }
}
