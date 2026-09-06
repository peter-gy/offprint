# Testing and validation

Tests protect public behavior, ownership, security, and package boundaries.
Choose the nearest supported boundary and keep resource-heavy browser work
explicit.

## Main commands

```console
just fmt-check
just repo-check
just workflow-check
just lint
just test
just codegen-check
just collector-check
just node-check
just node-package-check
just python-check
just python-wheel-check
just docs-check
just release-check
```

`just clean` removes Cargo targets, JavaScript installations, Python virtual
environments and caches, package output, native addons, and other generated
residue.

`just docs-check` verifies that every public and development page is routed
from its index, every local Markdown target exists, text satisfies repository
rules, Rust documentation builds with warnings denied, and Rust examples pass
as doctests. Binding examples are exercised by `node-check`,
`node-package-check`, `python-check`, and `python-wheel-check`.

## Browser fixtures

Use one owned fixture during development:

```console
just test-fixture FIXTURE_ID
```

Run one serialized group before handoff:

```console
just e2e GROUP
```

Fixture selection fails when an ID resolves to zero or multiple test owners.
Browser fixtures run with one test thread. Every external wait needs a deadline.

The fixture server provides controlled HTTP and HTTPS origins, redirects,
authentication, frames, service workers, compression, partial streams,
WebSocket and EventSource behavior, and oversized inputs.

## Boundary selection

| Change | Required evidence |
| --- | --- |
| Public record or schema | Unit tests, codegen freshness, binding contracts |
| Browser adapter | Focused test plus browser fixture group and process cleanup |
| Parser, resource graph, serializer, or protocol | Focused tests and `just fuzz-smoke` |
| Foreign-function-interface-free ownership or state | Focused tests and `just miri` |
| Node.js or Python | Source tests and extracted package install capture |
| Export format | Encoder verifier tests, artifact export integration, package mapping |
| Output transaction | Failure injection, conflict modes, recovery, package path |
| Measured capture path | Benchmark comparison and repeated-capture evidence |
| Workflow or release | Workflow lint, repository checks, package dry runs |

Package checks build and exercise extracted package graphs. Source-tree tests
do not establish package contents.

## Lifecycle review

Trace success, failure, cancellation, consumer drop, encoding failure,
verification failure, shutdown, and host exit. Confirm one terminal owner for:

- Browser process and remote connection
- Managed revision lease
- Context and page
- Target and transport task
- Event producer and subscriber
- Resource stream and content-store entry
- Temporary artifact and staging output
- Multi-output journal and backup

## Current conformance gap

The repository exercises Chromium, custom backends, and every representation
through focused tests. It does not yet expose the shared reusable backend and
representation conformance suites named by `AGENTS.md`. Keep that gap visible
until the common harnesses exist.
