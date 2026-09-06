# Offprint development documentation

These pages describe the current implementation, ownership boundaries, and
maintenance workflows. User-facing behavior lives in [`docs/`](../docs/README.md).

## Product and architecture

1. [Product contract](./product-contract.md) defines current scope, nouns, and
   invariants.
2. [Architecture](./architecture.md) maps crates, ports, adapters, and dependency
   direction.
3. [Capture pipeline](./capture-pipeline.md) traces the two-page browser
   lifecycle through commit.
4. [Browser and network](./browser-and-network.md) covers discovery, managed
   installation, remote CDP, containment, and ownership.
5. [Artifacts and formats](./artifacts-and-formats.md) covers verified-value
   boundaries, format encoders, and transactions.
6. [Schedulers](./schedulers.md) covers batch, crawl, plan hashes, and resume
   manifests.
7. [Bindings](./bindings.md) covers Node.js and Python mapping, lifecycle, and
   generated contracts.
8. [Document and collector](./document-and-collector.md) covers page observation,
   the arena model, recursive resources, and safe-static transformation.

## Contracts and maintenance

- [Schemas and versioning](./schemas-and-versioning.md)
- [Configuration and diagnostics](./configuration-and-diagnostics.md)
- [Contributor setup](./setup.md)
- [Generated files](./generated-files.md)
- [Testing](./testing.md)
- [Dependency policy](./dependencies.md)
- [Performance evidence](./performance.md)
- [Source and license provenance](./provenance.md)
- [Architecture decisions](./decisions.md)
- [Release process](./release.md)
- [Review checklist](./review.md)

## Change routing

| Change | Primary owner | Required companion review |
| --- | --- | --- |
| Public record or default | `offprint-model` | Schemas, bindings, CLI JSON, docs, [Semantic Versioning](https://semver.org/) review |
| Browser observation or lifecycle | `offprint-browser`, `offprint-chromium` | Network policy, owner cleanup, browser fixtures |
| Capture state or budget | `offprint-capture`, `offprint` | Cancellation, events, limit accounting |
| Document or safe-static behavior | `offprint-document`, `offprint-html`, `offprint-transform` | Recursive resources, static and offline verification |
| Export format | `offprint-export`, `offprint-artifact`, `offprint` | Format verifier, output transaction, bindings, docs |
| CLI or configuration | `offprint-cli` | stdout, stderr, JSON, exit status, precedence |
| Node.js or Python | Binding package | Generated record parity, package install, lifecycle |
| Release or package layout | `xtask`, manifests, workflows | Extracted package tests and public install smoke |

Start with the smallest focused check, then use the commands assigned to the
affected boundary in [Testing](./testing.md).
