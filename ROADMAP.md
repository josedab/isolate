# Roadmap

This document outlines the public roadmap for the Isolate project. It reflects our current priorities and planned direction, but is subject to change as we learn from community feedback and evolving requirements.

## Current Status

**v0.1.0** — Initial release with core sandbox functionality, capability-based security, resource limits, epoch-based timeout interruption, and WASI Preview 1 support.

## v0.2 Milestones

### Core Runtime

- [ ] WASI Preview 2 support
- [ ] Snapshot/restore stability
- [ ] Improved cold start times

### Security

- [ ] Seccomp-bpf integration (Linux)
- [ ] Landlock LSM support
- [ ] Module signing verification

### Developer Experience

- [ ] Python SDK GA
- [ ] Java SDK GA
- [ ] Improved error messages
- [ ] CLI improvements

### Observability

- [ ] OpenTelemetry tracing
- [ ] Built-in metrics dashboard
- [ ] Resource usage reporting

### Performance

- [ ] Module compilation caching
- [ ] Warm pool improvements
- [ ] Parallel execution

## Future Directions (v0.3+)

- **GPU compute acceleration** — Enable WASM sandboxes to offload work to GPU
  devices for ML inference, image processing, and parallel compute workloads.
  Requires a capability-gated GPU access model to maintain the security boundary.

- **Distributed mesh execution** — Allow sandboxes to span multiple nodes,
  enabling horizontal scaling for long-running or resource-intensive workloads.
  Builds on the gRPC server for inter-node communication and sandbox migration.

- **Component model composition** — Adopt the WASM Component Model to allow
  sandboxes to import and compose typed interfaces from other components, enabling
  modular plugin architectures without shared-memory coupling.

- **AI/ML workload optimization** — Specialized resource profiles and scheduling
  for inference workloads including batched execution, model weight caching, and
  optimized memory layouts for tensor operations.

## Feedback

We welcome input on our roadmap. Please share your thoughts and priorities through [GitHub Issues](https://github.com/josedab/isolate/issues) and [GitHub Discussions](https://github.com/josedab/isolate/discussions).
