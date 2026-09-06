# Compatibility and support

Offprint is currently an alpha project. Public APIs, schema records, artifact
formats, and package layout can change before the first stable release.

## Host support

| Surface | Supported baseline |
| --- | --- |
| Rust | Rust 1.97, edition 2024 |
| CLI archives | Linux x86-64 glibc, macOS arm64, macOS x86-64, Windows x86-64 |
| Node.js | Node.js 22 or newer on the four archive targets |
| Python | Python 3.10 through 3.14 wheels using Python's [stable application binary interface](https://docs.python.org/3/c-api/stable.html) on the four archive targets |
| System browser | Chrome, Chromium, or Edge based on Chromium 120 or newer |

Other source builds can use a compatible caller-selected system browser when
the managed catalog has no archive for the host.

## Independent version axes

| Axis | Current owner | Meaning |
| --- | --- | --- |
| Product version | `versions.toml` and package manifests | Rust crates, CLI, npm, and Python release version |
| Public schema | `offprint_model::PUBLIC_SCHEMA_VERSION` | Serialized request, event, result, error, and browser records |
| Artifact format | `offprint_model::ARTIFACT_FORMAT_VERSION` | Canonical Offprint HTML envelope |
| Collector protocol | `offprint-protocol` | Host-to-page observation messages |
| Collector bundle | Reserved `versions.toml` value | Injected JavaScript build. The value is not yet enforced |
| Browser catalog | Managed Chromium catalog | Pinned revisions and archive digests |
| CDP revision | Generated protocol input | Chrome DevTools Protocol definitions |

Current public schema and artifact format versions are both 2. Top-level
capture, batch, crawl, and export requests reject unknown fields and
incompatible schema versions. Nested closure is record-specific. A generated
schema with `additionalProperties: false` rejects extra fields. Verifiers reject
incompatible artifact format or schema versions.

## Interface parity

All interfaces share the native service, canonical record serialization,
capture defaults, artifact formats, and error codes. Host ergonomics differ:

- Rust exposes fluent builders, memory output, custom network rules, and
  browser ports directly.
- Node.js exposes TypeScript record aliases and one `OffprintError` class.
- Python uses snake_case method arguments and stage-specific exception classes.
- CLI JSON projects verification results into `ArtifactVerification` and uses
  process exit statuses.

Binding tests cover capture, inspection, export, format verification, events,
cancellation, errors, and shutdown. Shared Rust integration tests own broader
batch, crawl, browser-management, remote CDP, and artifact verification
evidence.
