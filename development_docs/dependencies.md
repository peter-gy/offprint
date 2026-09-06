# Dependency policy

Dependencies stay behind the crate that owns their behavior.

| Dependency | Boundary | Responsibility |
| --- | --- | --- |
| [Tokio](https://tokio.rs/) | Service, browser, I/O | Scheduling, cancellation, process I/O, bounded channels |
| [reqwest](https://docs.rs/reqwest/) with [rustls](https://docs.rs/rustls/) | Managed browser, remote CDP | Browser archive download and remote endpoint discovery |
| [tokio-tungstenite](https://docs.rs/tokio-tungstenite/) | Chromium transport | CDP WebSocket messages |
| [processkit](https://crates.io/crates/processkit) | Chromium launch | Descendant containment and shutdown |
| [html5ever](https://github.com/servo/html5ever) and markup5ever | Document and verification | Browser-compatible HTML parsing into controlled trees |
| [Lightning CSS](https://lightningcss.dev/) | Stylesheet discovery | Typed stylesheet and dependency inventory |
| [cssparser](https://docs.rs/cssparser/) | Preservation fallback | Reference locations with source-range preservation |
| [Serde](https://serde.rs/) and [Schemars](https://docs.rs/schemars/) | Canonical records | Serialization, examples, and JSON Schema |
| [napi-rs](https://napi.rs/) | Node.js binding | Native async host objects through Node-API |
| [PyO3](https://pyo3.rs/) and pyo3-async-runtimes | Python binding | Asyncio services and stable-ABI extension |
| [tempfile](https://docs.rs/tempfile/) | Content and transactions | Private staging with drop cleanup |
| [sha2](https://docs.rs/sha2/) and [crc32fast](https://docs.rs/crc32fast/) | Content and protocol | SHA-256 provenance and per-chunk corruption checks |

## Rules

- Workspace dependencies use locked resolution.
- Production crates reject wildcard versions.
- New dependencies belong to the narrowest workspace member that consumes
  them.
- Browser, parser, foreign-function interface, archive, and protocol changes
  run focused security and boundary checks.
- `cargo deny` checks advisories, licenses, registries, and Git sources.
- `cargo machete` checks unused Rust dependencies.
- Bun and Python lockfiles remain committed and install with frozen resolution.
- Review package lifecycle scripts before enabling them in a development or
  release environment.

The generated CDP layer keeps protocol-code generation out of the production
runtime.
