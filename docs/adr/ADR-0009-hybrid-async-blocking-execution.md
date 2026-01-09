# ADR-0009: Hybrid Async/Blocking Execution Model

## Status

Accepted

## Context

Isolate faces a tension between two execution models:

1. **Async (Tokio)**: Needed for I/O operations, timeouts, concurrent sandbox management, and integration with async Rust ecosystem
2. **Blocking (sync)**: WASM execution via Wasmtime is inherently blocking - it runs compiled machine code on the CPU

Forcing one model creates problems:

- Pure async: Would require spawning blocking tasks constantly, complex synchronization
- Pure blocking: Would block the async runtime, preventing concurrent operations

We needed an execution model that:

- Allows async APIs for sandbox management
- Handles blocking WASM execution efficiently
- Supports async timeout mechanisms
- Enables concurrent sandbox operations

## Decision

We implemented a **hybrid async/blocking execution model** using Tokio's `spawn_blocking` for WASM execution within an async API surface.

### API Surface

The public API is async:

```rust
impl Sandbox {
    pub async fn create(config: SandboxConfig) -> Result<Self> {
        // Module compilation can be CPU-intensive
        Self::create_with_engine(config, Arc::new(WasmEngine::new()?)).await
    }

    pub async fn run(&mut self, input: &[u8]) -> Result<Output> {
        self.ensure_state(SandboxState::Ready)?;
        self.state = SandboxState::Running;

        // ... setup code ...

        // WASM execution is blocking - offload to thread pool
        let result = tokio::task::spawn_blocking(move || instance.run())
            .await
            .map_err(|e| Error::Execution(e.to_string()))?;

        // ... handle result ...
    }

    pub async fn terminate(&mut self) -> Result<SandboxMetrics> {
        // Cleanup is fast, can be async
    }
}
```

### Async Timeout with Epoch Interruption

The timeout mechanism combines async and blocking:

```rust
pub async fn run(&mut self, input: &[u8]) -> Result<Output> {
    // Set up epoch-based timeout
    const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);

    if let Some(timeout) = self.config.resources.time.wall_time {
        let epochs_until_timeout =
            (timeout.as_millis() / EPOCH_TICK_INTERVAL.as_millis()) as u64;
        instance.set_epoch_deadline(epochs_until_timeout);

        // Spawn async task to increment epochs
        let engine = self.engine.clone();
        let cancel_token = CancellationToken::new();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(EPOCH_TICK_INTERVAL);
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    _ = interval.tick() => engine.increment_epoch(),
                }
            }
        });
    }

    // Blocking WASM execution
    let result = tokio::task::spawn_blocking(move || instance.run()).await?;

    // Cancel epoch ticker
    cancel_token.cancel();
}
```

### Async Instance Management

Instance pooling and management are async:

```rust
impl WarmPool {
    pub async fn acquire(&self) -> Result<PooledSandbox> {
        // Async lock, doesn't block other tasks
        let mut instances = self.instances.lock().await;
        // ...
    }

    pub async fn return_instance(&self, sandbox: PooledSandbox) {
        // ...
    }
}
```

## Consequences

### Positive

- **Best of both worlds**: Async APIs with efficient blocking execution
- **Non-blocking management**: Sandbox creation, pooling, monitoring don't block
- **Timeout support**: Epoch ticker runs asynchronously
- **Scalable**: Thread pool manages blocking work, doesn't starve async tasks
- **Familiar patterns**: Standard async Rust API surface

### Negative

- **Thread pool overhead**: `spawn_blocking` has context switch cost
- **Complexity**: Must carefully separate async and blocking code
- **Potential deadlocks**: Blocking in async context is risky (hence `spawn_blocking`)
- **Runtime dependency**: Requires Tokio runtime

### Implications

- Public async functions should never call blocking code directly
- WASM execution must always go through `spawn_blocking`
- Shared state between async and blocking contexts needs `Arc`
- Tests need `#[tokio::test]` attribute
- The epoch ticker pattern should be reused for any async interruption need
