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

- GPU compute acceleration
- Distributed mesh execution
- Component model composition
- AI/ML workload optimization

## Feedback

We welcome input on our roadmap. Please share your thoughts and priorities through [GitHub Issues](https://github.com/isolate/isolate/issues) and [GitHub Discussions](https://github.com/isolate/isolate/discussions).
