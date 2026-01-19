# Add Documentation Comments to Capability Variants

## Task Description

Add comprehensive doc comments to all `Capability` variants and their associated
functions. Good documentation helps users understand what each capability grants.

## Background Context

The capability system is a core security feature. Each capability grants specific
permissions to a sandbox. Users need to understand:
- What the capability permits
- Security implications
- Common use cases
- Related capabilities

Currently, some variants have minimal or no documentation.

## Files to Modify

- `isolate-core/src/capability/types.rs` - Main capability definitions
- `isolate-core/src/capability/mod.rs` - Module-level docs if needed

## Acceptance Criteria

- [ ] All `Capability` enum variants have doc comments
- [ ] Doc comments explain what each capability permits
- [ ] Security considerations are mentioned where relevant
- [ ] Examples are provided for common capabilities
- [ ] `cargo doc --open` shows well-formatted documentation
- [ ] No `missing_docs` warnings for capability module

## Example Documentation Style

```rust
/// Grants permission to write to the standard output stream.
///
/// This capability allows the sandbox to print output that will be
/// captured and returned in [`Output::stdout`].
///
/// # Security Considerations
///
/// Stdout output is captured but limited by I/O write limits.
/// Large amounts of output may be truncated.
///
/// # Example
///
/// ```rust
/// use isolate_core::capability::Capability;
///
/// let cap = Capability::stdout();
/// ```
Stdout,
```

## Helpful Resources

- Rust documentation guidelines: https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html
- Existing capability docs in the module
- WASI capability concepts

## Estimated Difficulty

Easy (< 1 hour)
