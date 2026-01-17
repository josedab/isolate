# Reference Architectures

Production-tested architecture patterns for building systems with Isolate.

| Architecture | Use Case | Complexity |
|-------------|----------|------------|
| [Serverless FaaS Platform](faas-platform.md) | AWS Lambda alternative | Medium |
| [SaaS Plugin System](saas-plugins.md) | User scripts in your product | Low |
| [Edge Compute](edge-compute.md) | CDN edge functions | Medium |
| [CI/CD Secure Runner](cicd-runner.md) | Untrusted build scripts | Low |
| [Multi-Tenant Analytics](analytics-udfs.md) | User-defined functions | Medium |

## Choosing an Architecture

- **Need simple script execution?** → Start with [SaaS Plugin System](saas-plugins.md)
- **Building a compute platform?** → See [Serverless FaaS](faas-platform.md)
- **Running at the edge?** → See [Edge Compute](edge-compute.md)
- **Need secure CI/CD?** → See [CI/CD Runner](cicd-runner.md)
- **Custom analytics?** → See [Analytics UDFs](analytics-udfs.md)
