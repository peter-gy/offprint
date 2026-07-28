# Dependency evaluation

PageKnot keeps public records in `pageknot-model` and places runtime
dependencies behind narrow crate boundaries.

| Dependency | Boundary | Decision |
| --- | --- | --- |
| Tokio | service, browser, and I/O crates | Own async scheduling, cancellation, process I/O, and bounded channels |
| reqwest with rustls | managed browser and HTTP retrieval | Use a platform-consistent TLS client with streaming bodies |
| tokio-tungstenite | Chromium transport | Carry CDP messages over local or remote WebSocket endpoints |
| processkit | Chromium launch | Own descendant containment and shutdown across supported hosts |
| html5ever and markup5ever | document and verification | Parse browser-corrected HTML into a controlled arena and reparse artifacts |
| Lightning CSS | stylesheet discovery | Parse complete stylesheets and classify the typed dependency inventory |
| cssparser | CSS preservation fallback | Locate render-affecting references while preserving source ranges exactly |
| serde and schemars | canonical records | Generate JSON records, examples, and schemas from one Rust model |
| napi-rs | Node.js binding | Expose async Rust services through stable Node-API |
| PyO3 and pyo3-async-runtimes | Python binding | Expose asyncio jobs while releasing the Python runtime lock during Rust work |
| tempfile | content store and transactions | Create private same-filesystem staging entries with drop cleanup |
| sha2 and crc32fast | content and protocol integrity | Use SHA-256 for provenance and CRC32 for per-chunk transport corruption |

## Dependency rules

- Workspace dependencies use exact lockfile resolution.
- Production crates reject wildcard dependencies.
- Browser, parser, FFI, and archive changes run their focused fixture or fuzz
  suite.
- `cargo deny` checks advisories, licenses, registries, and Git sources.
- `cargo machete` checks unused Rust dependencies.
- Collector and binding lockfiles are committed and installed with frozen
  resolution in CI.

The generated CDP layer keeps the Chromium protocol dependency to selected
domains and avoids a runtime protocol-codegen dependency.
