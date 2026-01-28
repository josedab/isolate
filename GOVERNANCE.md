# Governance

This document describes the governance model for the Isolate project.

## Project Roles

### Maintainer

**Current Maintainer:** [@josedab](https://github.com/josedab)

The maintainer is responsible for:
- Setting project direction and priorities
- Reviewing and merging pull requests
- Managing releases
- Ensuring code quality and security
- Growing the contributor community

### Contributors

Anyone who contributes code, documentation, or other improvements to the project.
Contributors are the lifeblood of open source.

### Area Maintainers (Open Positions)

We're looking for area maintainers to help grow the project. Areas include:

| Area | Status | Responsibilities |
|------|--------|------------------|
| Documentation | **Open** | Improve docs, tutorials, examples |
| Security | **Open** | Security audits, vulnerability triage |
| Performance | **Open** | Benchmarks, optimization |
| Python Bindings | **Open** | isolate-python development |
| Community | **Open** | Issue triage, helping newcomers |

Interested? Open an issue or reach out directly.

## Decision Making

### Technical Decisions

- **Minor changes:** Single maintainer approval
- **Significant changes:** Discussion in issue/PR, maintainer decision
- **Architecture changes:** RFC process (see below)

### RFC (Request for Comments) Process

Architectural or cross-cutting changes require an RFC before implementation:

1. **Open an issue** titled `RFC: <short description>` with the `RFC` label
2. **Fill in the RFC template** (see below)
3. **Discussion period** — minimum 7 days for community feedback
4. **Decision** — maintainer accepts, requests revisions, or declines
5. **Implementation** — once accepted, create a tracking issue and submit PRs
6. **ADR** — record the decision in `docs/adr/` for long-term reference

#### When to write an RFC

- Adding or removing a public API surface
- Changing the security model or capability system
- Modifying the sandbox lifecycle or execution model
- Introducing a new runtime dependency
- Any change that affects multiple crates in the workspace

#### RFC Template

See [`docs/RFC_TEMPLATE.md`](docs/RFC_TEMPLATE.md) for the full template.

```markdown
# RFC: <Title>

## Summary
One paragraph explanation of the change.

## Motivation
Why are we doing this? What problem does it solve?

## Design
Detailed description of the proposed change including API surface,
data structures, and interaction with existing components.

## Alternatives Considered
What other approaches were evaluated and why were they rejected?

## Drawbacks
What are the tradeoffs or downsides?

## Unresolved Questions
What aspects of the design are still open?
```

### Adding New Features

1. Open an issue describing the feature
2. Discuss design and implementation approach
3. Submit PR referencing the issue
4. Code review and iteration
5. Merge when approved

### Breaking Changes

Breaking changes require:
- Clear documentation of what breaks
- Migration guide when possible
- Discussion period before merging
- Semantic version bump

## Path to Maintainer

Contributors can become area maintainers by:

1. **Consistent contributions** - Multiple quality PRs over time
2. **Domain expertise** - Deep understanding of the area
3. **Community engagement** - Helping others, reviewing PRs
4. **Reliability** - Following through on commitments

To express interest:
1. Open an issue titled "Area Maintainer Interest: [Area]"
2. Describe your relevant experience
3. Link to your contributions

## Code of Conduct

All participants must follow the project's [Code of Conduct](CODE_OF_CONDUCT.md).

## Communication

- **Issues:** Feature requests, bug reports, discussions
- **Pull Requests:** Code contributions
- **Discussions:** Q&A, ideas, show and tell (when enabled)

## Releases

The maintainer controls releases. The release process:

1. Ensure CI passes on main
2. Update CHANGELOG.md
3. Bump version in Cargo.toml files
4. Create git tag
5. GitHub Actions handles publishing

### Versioning

We follow [Semantic Versioning](https://semver.org/):
- **Major:** Breaking changes
- **Minor:** New features (backwards compatible)
- **Patch:** Bug fixes (backwards compatible)

## Security

Security issues should be reported privately. See [SECURITY.md](SECURITY.md).

## Changes to Governance

Changes to this document require:
1. Issue discussing the proposed change
2. Community feedback period (7 days minimum)
3. Maintainer approval

---

*This governance model is intentionally lightweight for a young project.
It will evolve as the community grows.*
