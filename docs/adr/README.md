# Architecture Decision Records (ADRs)

This directory contains Architecture Decision Records (ADRs) documenting significant architectural decisions made in the Isolate project.

## What is an ADR?

An Architecture Decision Record captures a single architecture decision with its context and consequences. ADRs are numbered sequentially and are immutable once accepted—new decisions may supersede old ones, but the original record remains for historical reference.

## ADR Index

| ADR | Title | Status |
|-----|-------|--------|
| [ADR-0001](ADR-0001-wasmtime-as-wasm-runtime.md) | Wasmtime as WASM Runtime | Accepted |
| [ADR-0002](ADR-0002-capability-based-security-model.md) | Capability-Based Security Model | Accepted |
| [ADR-0003](ADR-0003-multi-dimensional-resource-limiting.md) | Multi-Dimensional Resource Limiting | Accepted |
| [ADR-0004](ADR-0004-shared-engine-with-module-caching.md) | Shared Engine with Module Caching | Accepted |
| [ADR-0005](ADR-0005-explicit-state-machine-for-sandbox-lifecycle.md) | Explicit State Machine for Sandbox Lifecycle | Accepted |
| [ADR-0006](ADR-0006-hierarchical-error-types-with-categorization.md) | Hierarchical Error Types with Categorization | Accepted |
| [ADR-0007](ADR-0007-copy-on-write-snapshot-persistence.md) | Copy-on-Write Snapshot Persistence | Accepted |
| [ADR-0008](ADR-0008-kubernetes-native-orchestration.md) | Kubernetes-Native Orchestration via CRDs | Accepted |
| [ADR-0009](ADR-0009-hybrid-async-blocking-execution.md) | Hybrid Async/Blocking Execution Model | Accepted |
| [ADR-0010](ADR-0010-builder-pattern-for-configuration.md) | Builder Pattern for Configuration | Accepted |
| [ADR-0011](ADR-0011-dual-metrics-architecture.md) | Dual Metrics Architecture | Accepted |
| [ADR-0012](ADR-0012-parking-lot-and-atomics-for-concurrency.md) | Parking-lot and Atomics for Concurrency | Accepted |

## ADR Categories

### Core Runtime
- **ADR-0001**: Wasmtime selection for WASM execution
- **ADR-0004**: Engine sharing and module caching strategy
- **ADR-0005**: Sandbox state machine design
- **ADR-0009**: Async/blocking execution model

### Security & Isolation
- **ADR-0002**: Capability-based security model
- **ADR-0003**: Resource limiting (CPU, memory, I/O)

### Data & State
- **ADR-0007**: Snapshot persistence with copy-on-write

### Operations & Observability
- **ADR-0008**: Kubernetes orchestration approach
- **ADR-0011**: Metrics collection architecture

### Code Quality
- **ADR-0006**: Error type hierarchy
- **ADR-0010**: Builder pattern for configuration
- **ADR-0012**: Concurrency primitives selection

## ADR Template

New ADRs should follow this structure:

```markdown
# ADR-XXXX: Title

## Status

[Proposed | Accepted | Deprecated | Superseded by ADR-XXXX]

## Context

[Describe the forces at play, including technological, political, social, and project local.]

## Decision

[Describe the decision and the reasons for making it.]

## Consequences

### Positive
[List positive outcomes]

### Negative
[List negative outcomes or tradeoffs]

### Implications
[List any ongoing implications for the team]
```

## Creating a New ADR

1. Create a new file: `ADR-XXXX-short-title.md`
2. Use the next sequential number
3. Fill in the template sections
4. Submit a PR for review
5. Update this README with the new ADR entry

## References

- [Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) - Michael Nygard
- [ADR GitHub Organization](https://adr.github.io/)
