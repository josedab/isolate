# Implement Display for SandboxState

## Task Description

Implement the `std::fmt::Display` trait for `SandboxState` to provide
human-readable string representations.

## Background Context

`SandboxState` represents the lifecycle state of a sandbox (Created, Running,
Completed, Failed, etc.). While `Debug` is derived automatically, a custom
`Display` implementation provides cleaner output for logs and user-facing messages.

## Files to Modify

- `isolate-core/src/sandbox.rs` - Add Display impl for SandboxState

## Acceptance Criteria

- [ ] `Display` trait implemented for `SandboxState`
- [ ] Output is concise and human-readable
- [ ] Works correctly with format strings: `format!("{}", state)`
- [ ] Unit test added for Display implementation
- [ ] No breaking changes to existing API

## Example Implementation

```rust
impl std::fmt::Display for SandboxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxState::Created => write!(f, "created"),
            SandboxState::Running => write!(f, "running"),
            SandboxState::Completed { exit_code } => {
                write!(f, "completed (exit: {})", exit_code)
            }
            SandboxState::Failed { error } => {
                write!(f, "failed: {}", error)
            }
            // ... other variants
        }
    }
}
```

## Helpful Resources

- std::fmt::Display documentation: https://doc.rust-lang.org/std/fmt/trait.Display.html
- Similar implementations in the codebase (e.g., Error types)

## Estimated Difficulty

Easy (< 1 hour)
