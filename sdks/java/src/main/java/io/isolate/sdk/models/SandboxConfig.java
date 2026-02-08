package io.isolate.sdk.models;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Configuration for creating a new sandbox.
 *
 * <p>Use the {@link #builder()} method to construct instances. All fields have
 * sensible defaults: zero-valued limits mean the server will apply its own
 * defaults.</p>
 *
 * <pre>{@code
 * SandboxConfig config = SandboxConfig.builder()
 *     .memoryLimit(64 * 1024 * 1024)
 *     .fuelLimit(1_000_000)
 *     .wallTimeLimitSecs(30)
 *     .addCapability("stdout", "")
 *     .addCapability("fs_read", "/data")
 *     .putEnv("MODE", "production")
 *     .build();
 * }</pre>
 */
public final class SandboxConfig {

    private final long memoryLimit;
    private final long fuelLimit;
    private final int wallTimeLimitSecs;
    private final int cpuTimeLimitSecs;
    private final List<Capability> capabilities;
    private final Map<String, String> env;
    private final List<String> args;

    private SandboxConfig(Builder builder) {
        this.memoryLimit = builder.memoryLimit;
        this.fuelLimit = builder.fuelLimit;
        this.wallTimeLimitSecs = builder.wallTimeLimitSecs;
        this.cpuTimeLimitSecs = builder.cpuTimeLimitSecs;
        this.capabilities = Collections.unmodifiableList(new ArrayList<>(builder.capabilities));
        this.env = Collections.unmodifiableMap(new LinkedHashMap<>(builder.env));
        this.args = Collections.unmodifiableList(new ArrayList<>(builder.args));
    }

    /**
     * Returns a new builder for {@link SandboxConfig}.
     *
     * @return a new builder instance
     */
    public static Builder builder() {
        return new Builder();
    }

    /**
     * Returns the memory limit in bytes. A value of 0 means the server default is used.
     *
     * @return memory limit in bytes
     */
    public long getMemoryLimit() {
        return memoryLimit;
    }

    /**
     * Returns the fuel limit. A value of 0 means the server default is used.
     *
     * @return fuel limit
     */
    public long getFuelLimit() {
        return fuelLimit;
    }

    /**
     * Returns the wall-clock time limit in seconds. A value of 0 means the server default is used.
     *
     * @return wall time limit in seconds
     */
    public int getWallTimeLimitSecs() {
        return wallTimeLimitSecs;
    }

    /**
     * Returns the CPU time limit in seconds. A value of 0 means the server default is used.
     *
     * @return CPU time limit in seconds
     */
    public int getCpuTimeLimitSecs() {
        return cpuTimeLimitSecs;
    }

    /**
     * Returns an unmodifiable list of capabilities granted to the sandbox.
     *
     * @return capabilities list
     */
    public List<Capability> getCapabilities() {
        return capabilities;
    }

    /**
     * Returns an unmodifiable map of environment variables for the sandbox.
     *
     * @return environment variables
     */
    public Map<String, String> getEnv() {
        return env;
    }

    /**
     * Returns an unmodifiable list of command-line arguments for the sandbox.
     *
     * @return arguments list
     */
    public List<String> getArgs() {
        return args;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (o == null || getClass() != o.getClass()) return false;
        SandboxConfig that = (SandboxConfig) o;
        return memoryLimit == that.memoryLimit
                && fuelLimit == that.fuelLimit
                && wallTimeLimitSecs == that.wallTimeLimitSecs
                && cpuTimeLimitSecs == that.cpuTimeLimitSecs
                && Objects.equals(capabilities, that.capabilities)
                && Objects.equals(env, that.env)
                && Objects.equals(args, that.args);
    }

    @Override
    public int hashCode() {
        return Objects.hash(memoryLimit, fuelLimit, wallTimeLimitSecs, cpuTimeLimitSecs,
                capabilities, env, args);
    }

    @Override
    public String toString() {
        return "SandboxConfig{"
                + "memoryLimit=" + memoryLimit
                + ", fuelLimit=" + fuelLimit
                + ", wallTimeLimitSecs=" + wallTimeLimitSecs
                + ", cpuTimeLimitSecs=" + cpuTimeLimitSecs
                + ", capabilities=" + capabilities
                + ", env=" + env
                + ", args=" + args
                + '}';
    }

    /**
     * A capability granted to a sandbox.
     *
     * <p>Each capability has a type (e.g., "stdout", "fs_read", "net_connect")
     * and an optional value (e.g., a filesystem path or host).</p>
     */
    public static final class Capability {

        private final String type;
        private final String value;

        /**
         * Creates a new capability.
         *
         * @param type  the capability type
         * @param value the capability value (path, host, etc.), or empty string
         */
        public Capability(String type, String value) {
            this.type = Objects.requireNonNull(type, "type must not be null");
            this.value = Objects.requireNonNull(value, "value must not be null");
        }

        /**
         * Returns the capability type.
         *
         * @return the type
         */
        public String getType() {
            return type;
        }

        /**
         * Returns the capability value.
         *
         * @return the value
         */
        public String getValue() {
            return value;
        }

        @Override
        public boolean equals(Object o) {
            if (this == o) return true;
            if (o == null || getClass() != o.getClass()) return false;
            Capability that = (Capability) o;
            return Objects.equals(type, that.type) && Objects.equals(value, that.value);
        }

        @Override
        public int hashCode() {
            return Objects.hash(type, value);
        }

        @Override
        public String toString() {
            return "Capability{type='" + type + "', value='" + value + "'}";
        }
    }

    /**
     * Builder for {@link SandboxConfig}.
     */
    public static final class Builder {

        private long memoryLimit;
        private long fuelLimit;
        private int wallTimeLimitSecs;
        private int cpuTimeLimitSecs;
        private final List<Capability> capabilities = new ArrayList<>();
        private final Map<String, String> env = new LinkedHashMap<>();
        private final List<String> args = new ArrayList<>();

        private Builder() {
        }

        /**
         * Sets the memory limit in bytes.
         *
         * @param memoryLimit memory limit in bytes (must be non-negative)
         * @return this builder
         * @throws IllegalArgumentException if memoryLimit is negative
         */
        public Builder memoryLimit(long memoryLimit) {
            if (memoryLimit < 0) {
                throw new IllegalArgumentException("memoryLimit must be non-negative, got: " + memoryLimit);
            }
            this.memoryLimit = memoryLimit;
            return this;
        }

        /**
         * Sets the fuel limit.
         *
         * @param fuelLimit fuel limit (must be non-negative)
         * @return this builder
         * @throws IllegalArgumentException if fuelLimit is negative
         */
        public Builder fuelLimit(long fuelLimit) {
            if (fuelLimit < 0) {
                throw new IllegalArgumentException("fuelLimit must be non-negative, got: " + fuelLimit);
            }
            this.fuelLimit = fuelLimit;
            return this;
        }

        /**
         * Sets the wall-clock time limit in seconds.
         *
         * @param wallTimeLimitSecs wall time limit in seconds (must be non-negative)
         * @return this builder
         * @throws IllegalArgumentException if wallTimeLimitSecs is negative
         */
        public Builder wallTimeLimitSecs(int wallTimeLimitSecs) {
            if (wallTimeLimitSecs < 0) {
                throw new IllegalArgumentException("wallTimeLimitSecs must be non-negative, got: " + wallTimeLimitSecs);
            }
            this.wallTimeLimitSecs = wallTimeLimitSecs;
            return this;
        }

        /**
         * Sets the CPU time limit in seconds.
         *
         * @param cpuTimeLimitSecs CPU time limit in seconds (must be non-negative)
         * @return this builder
         * @throws IllegalArgumentException if cpuTimeLimitSecs is negative
         */
        public Builder cpuTimeLimitSecs(int cpuTimeLimitSecs) {
            if (cpuTimeLimitSecs < 0) {
                throw new IllegalArgumentException("cpuTimeLimitSecs must be non-negative, got: " + cpuTimeLimitSecs);
            }
            this.cpuTimeLimitSecs = cpuTimeLimitSecs;
            return this;
        }

        /**
         * Adds a capability to grant to the sandbox.
         *
         * @param type  the capability type (e.g., "stdout", "fs_read", "net_connect")
         * @param value the capability value (e.g., a path or host), or empty string
         * @return this builder
         */
        public Builder addCapability(String type, String value) {
            this.capabilities.add(new Capability(type, value));
            return this;
        }

        /**
         * Adds a capability with no value.
         *
         * @param type the capability type
         * @return this builder
         */
        public Builder addCapability(String type) {
            return addCapability(type, "");
        }

        /**
         * Sets an environment variable for the sandbox.
         *
         * @param key   the variable name
         * @param value the variable value
         * @return this builder
         */
        public Builder putEnv(String key, String value) {
            this.env.put(
                    Objects.requireNonNull(key, "env key must not be null"),
                    Objects.requireNonNull(value, "env value must not be null")
            );
            return this;
        }

        /**
         * Sets all environment variables from the given map.
         *
         * @param env the environment variables
         * @return this builder
         */
        public Builder env(Map<String, String> env) {
            this.env.clear();
            if (env != null) {
                this.env.putAll(env);
            }
            return this;
        }

        /**
         * Adds a command-line argument.
         *
         * @param arg the argument
         * @return this builder
         */
        public Builder addArg(String arg) {
            this.args.add(Objects.requireNonNull(arg, "arg must not be null"));
            return this;
        }

        /**
         * Sets the command-line arguments.
         *
         * @param args the arguments list
         * @return this builder
         */
        public Builder args(List<String> args) {
            this.args.clear();
            if (args != null) {
                this.args.addAll(args);
            }
            return this;
        }

        /**
         * Builds the {@link SandboxConfig} instance.
         *
         * @return a new immutable SandboxConfig
         */
        public SandboxConfig build() {
            return new SandboxConfig(this);
        }
    }
}
