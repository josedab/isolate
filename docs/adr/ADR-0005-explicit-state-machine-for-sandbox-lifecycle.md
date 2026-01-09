# ADR-0005: Explicit State Machine for Sandbox Lifecycle

## Status

Accepted

## Context

Sandbox instances go through multiple phases: creation, compilation, execution, and termination. Without explicit state tracking, it's easy to accidentally:

- Call `run()` on an already running sandbox
- Attempt operations on terminated sandboxes
- Leave resources dangling in undefined states
- Produce confusing error messages for invalid state transitions

We needed a clear lifecycle model that:

- Prevents invalid operations based on current state
- Provides clear error messages for state violations
- Enables observability into sandbox status
- Supports future extensions like pause/resume

## Decision

We implemented an **explicit state machine** with five states and validated transitions.

### State Definitions

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    Creating,    // Sandbox is being initialized
    Ready,       // Sandbox is ready to run
    Running,     // Sandbox is currently executing
    Paused,      // Execution is suspended (future use)
    Terminated,  // Sandbox has completed or been terminated
}
```

### State Transitions

```
                 ┌──────────────┐
                 │   Creating   │
                 └──────┬───────┘
                        │ compile success
                        ▼
                 ┌──────────────┐
        ┌───────▶│    Ready     │◀───────┐
        │        └──────┬───────┘        │
        │               │ run()          │
        │ call()        ▼                │ resume()
        │        ┌──────────────┐        │
        │        │   Running    │────────┤
        │        └──────┬───────┘        │
        │               │                │
        │               ├──pause()──────▶│
        │               │                │
        │               │ complete/fail  │
        │               ▼                │
        │        ┌──────────────┐        │
        └────────│  Terminated  │        │
                 └──────────────┘        │
                        │                │
                        │                │
                 ┌──────────────┐        │
                 │    Paused    │────────┘
                 └──────────────┘
```

### State Validation

```rust
fn ensure_state(&self, expected: SandboxState) -> Result<()> {
    if self.state != expected {
        return Err(Error::InvalidState {
            expected: expected.to_string(),
            actual: self.state.to_string(),
        });
    }
    Ok(())
}

pub async fn run(&mut self, input: &[u8]) -> Result<Output> {
    self.ensure_state(SandboxState::Ready)?;
    self.state = SandboxState::Running;
    // ... execution logic
    self.state = SandboxState::Terminated;
    // ...
}
```

### State Observability

- `sandbox.state()` returns current state
- State transitions logged via tracing
- Metrics track state distribution across sandboxes
- API exposes state for monitoring/debugging

## Consequences

### Positive

- **Prevents invalid operations**: State checks catch programming errors early
- **Clear error messages**: `InvalidState { expected, actual }` tells developers exactly what went wrong
- **Debuggable**: State is always inspectable, logged on transitions
- **Extensible**: Adding new states (e.g., `Suspended`) is straightforward
- **Serializable**: State can be persisted with `#[derive(Serialize)]`

### Negative

- **Overhead**: Each operation has a state check (minimal cost)
- **Rigid**: Some valid operation sequences may be blocked (e.g., can't re-run terminated sandbox)
- **Single path**: Current design doesn't support branching execution

### Implications

- All sandbox methods must check and update state appropriately
- Tests should verify state transitions, not just outcomes
- Pool implementations must track state of pooled sandboxes
- Future snapshot/restore will need to serialize and restore state
