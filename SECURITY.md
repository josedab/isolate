# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

The Isolate project takes security seriously. If you discover a security vulnerability, please report it responsibly.

### How to Report

1. **Do NOT open a public GitHub issue** for security vulnerabilities
2. Email your findings to the maintainers (see repository for contact info)
3. Include the following in your report:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

### What to Expect

- **Acknowledgment**: Within 48 hours of your report
- **Initial Assessment**: Within 7 days
- **Resolution Timeline**: Depends on severity
  - Critical: 7 days
  - High: 14 days
  - Medium: 30 days
  - Low: 90 days

### Scope

Security issues we're interested in:

- Sandbox escape vulnerabilities
- Capability bypass issues
- Resource limit circumvention
- Memory safety issues in unsafe code (if any)
- Denial of service via crafted WASM modules
- Information disclosure between sandboxes
- Authentication/authorization issues in the gRPC server

### Out of Scope

- Vulnerabilities in dependencies (report to upstream)
- Issues requiring physical access
- Social engineering attacks
- Issues in experimental modules (gpu, mesh, enclave, hotpatch, verify, security)

### Safe Harbor

We consider security research conducted in good faith to be authorized. We will not pursue legal action against researchers who:

- Make a good faith effort to avoid privacy violations and data destruction
- Only access data necessary to demonstrate the vulnerability
- Report vulnerabilities promptly and do not exploit them
- Allow reasonable time for remediation before disclosure

## Security Best Practices

When using Isolate in production:

1. **Principle of Least Privilege**: Grant only necessary capabilities
2. **Resource Limits**: Always set appropriate memory, CPU, and I/O limits
3. **Input Validation**: Validate WASM modules before loading
4. **Audit Logging**: Enable audit logging for security-relevant operations
5. **Network Isolation**: Restrict network capabilities to specific hosts
6. **Regular Updates**: Keep Isolate and dependencies up to date

## Security Features

Isolate provides multiple layers of security:

- **WASM Sandbox**: Memory isolation, type-safe execution
- **Capability System**: Default-deny, explicit permission grants
- **Resource Limits**: Fuel metering, memory bounds, timeouts
- **Audit Logging**: Track all security-relevant operations
- **OS Integration** (Linux): seccomp-bpf, Landlock LSM support (experimental)
