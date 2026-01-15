package io.isolate.sdk;

import io.isolate.sdk.models.SandboxConfig;
import io.isolate.sdk.models.SandboxConfig.Capability;
import io.isolate.sdk.models.ResourceUsage;
import io.isolate.sdk.models.RunResult;
import io.isolate.sdk.models.SandboxInfo;
import io.isolate.sdk.exceptions.IsolateException;
import io.isolate.sdk.exceptions.SandboxNotFoundException;
import io.isolate.sdk.exceptions.SandboxExecutionException;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.DisplayName;

import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Unit tests for the Isolate Java SDK.
 *
 * Tests cover model construction, capability builders, configuration,
 * and exception hierarchy — all without requiring a running gRPC server.
 */
public class IsolateClientTest {

    // -----------------------------------------------------------------------
    // Capability tests
    // -----------------------------------------------------------------------

    @Test
    @DisplayName("Capability.stdout() creates stdout capability")
    void testCapabilityStdout() {
        Capability cap = Capability.stdout();
        assertEquals("stdout", cap.type());
        assertEquals("", cap.value());
    }

    @Test
    @DisplayName("Capability.stderr() creates stderr capability")
    void testCapabilityStderr() {
        Capability cap = Capability.stderr();
        assertEquals("stderr", cap.type());
    }

    @Test
    @DisplayName("Capability.fsRead() includes path")
    void testCapabilityFsRead() {
        Capability cap = Capability.fsRead("/data");
        assertEquals("fs_read", cap.type());
        assertEquals("/data", cap.value());
    }

    @Test
    @DisplayName("Capability.fsWrite() includes path")
    void testCapabilityFsWrite() {
        Capability cap = Capability.fsWrite("/output");
        assertEquals("fs_write", cap.type());
        assertEquals("/output", cap.value());
    }

    @Test
    @DisplayName("Capability.http() includes host")
    void testCapabilityHttp() {
        Capability cap = Capability.http("api.example.com");
        assertEquals("http", cap.type());
        assertEquals("api.example.com", cap.value());
    }

    @Test
    @DisplayName("Capability.env() includes variable name")
    void testCapabilityEnv() {
        Capability cap = Capability.env("API_KEY");
        assertEquals("env", cap.type());
        assertEquals("API_KEY", cap.value());
    }

    // -----------------------------------------------------------------------
    // SandboxConfig tests
    // -----------------------------------------------------------------------

    @Test
    @DisplayName("SandboxConfig builder creates valid config")
    void testSandboxConfigBuilder() {
        SandboxConfig config = SandboxConfig.builder()
                .memoryLimit(128 * 1024 * 1024)
                .fuel(1_000_000L)
                .capability(Capability.stdout())
                .capability(Capability.stderr())
                .env("KEY", "value")
                .build();

        assertNotNull(config);
    }

    @Test
    @DisplayName("SandboxConfig builder allows empty config")
    void testSandboxConfigEmpty() {
        SandboxConfig config = SandboxConfig.builder().build();
        assertNotNull(config);
    }

    // -----------------------------------------------------------------------
    // Exception hierarchy tests
    // -----------------------------------------------------------------------

    @Test
    @DisplayName("IsolateException is base exception")
    void testIsolateException() {
        IsolateException ex = new IsolateException("test error");
        assertEquals("test error", ex.getMessage());
        assertTrue(ex instanceof Exception);
    }

    @Test
    @DisplayName("SandboxNotFoundException extends IsolateException")
    void testSandboxNotFound() {
        SandboxNotFoundException ex = new SandboxNotFoundException("sb-123");
        assertTrue(ex instanceof IsolateException);
    }

    @Test
    @DisplayName("SandboxExecutionException extends IsolateException")
    void testSandboxExecution() {
        SandboxExecutionException ex = new SandboxExecutionException("execution failed");
        assertTrue(ex instanceof IsolateException);
    }

    // -----------------------------------------------------------------------
    // IsolateClientConfig tests
    // -----------------------------------------------------------------------

    @Test
    @DisplayName("IsolateClientConfig builder creates valid config")
    void testClientConfig() {
        IsolateClientConfig config = IsolateClientConfig.builder()
                .target("localhost:50051")
                .build();

        assertNotNull(config);
    }

    @Test
    @DisplayName("IsolateClientConfig with TLS")
    void testClientConfigTls() {
        IsolateClientConfig config = IsolateClientConfig.builder()
                .target("localhost:50051")
                .useTls(true)
                .build();

        assertNotNull(config);
    }
}
