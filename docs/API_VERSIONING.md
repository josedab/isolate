# API Versioning Policy

## Overview

The Isolate gRPC API uses protobuf package versioning (`isolate.v1`) and follows
a strict backward compatibility policy.

## Compatibility Guarantees

### Within a major version (v1)

- **New fields** may be added to existing messages at any time (always optional).
- **New RPC methods** may be added to existing services at any time.
- **Existing fields** will not be removed, renumbered, or have their type changed.
- **Deprecated fields** are marked with `[deprecated=true]` and kept for at
  least 2 minor releases before removal.

### Field number ranges

| Range | Purpose |
|-------|---------|
| 1–99 | Stable public fields |
| 100–199 | Reserved for future internal use |

### Breaking changes

A new major version (`isolate.v2`) will be created for:
- Removing fields or RPC methods
- Changing field types
- Renaming messages or services
- Altering RPC semantics

Both versions will be served simultaneously for at least one release cycle.

## Client expectations

- Clients **must** ignore unknown fields (standard protobuf behavior).
- Clients **should** check for new optional fields on each SDK upgrade.
- Clients **must not** depend on default values of optional fields remaining constant.
