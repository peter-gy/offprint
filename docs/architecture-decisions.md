# Accepted architecture decisions

Each record defines the current decision and the evidence that can reopen it.

## ADR 001: CLI-first native product

- **Context:** Capture needs browser processes, filesystem transactions, and
  predictable automation behavior.
- **Decision:** Ship a native CLI as the primary product surface.
- **Consequences:** stdout, stderr, exit status, signals, package archives, and
  clean-machine behavior are release contracts.
- **Rejected:** A hosted capture service as the primary runtime.
- **Reconsider with:** Evidence that a supported host cannot provide the
  required Chromium and transaction guarantees.

## ADR 002: Rust service API

- **Context:** The CLI and language bindings need one lifecycle and error model.
- **Decision:** Keep orchestration in the `pageknot` Rust service API.
- **Consequences:** Frontends map canonical records and do not own capture
  semantics.
- **Rejected:** Independent CLI, Node.js, and Python implementations.
- **Reconsider with:** A host requirement that cannot cross the service seam.

## ADR 003: Observational TypeScript collector

- **Context:** Browser state must be read where DOM and CSSOM APIs exist.
- **Decision:** Inject a thin collector that observes and serializes state.
- **Consequences:** Transformation and artifact policy stay in Rust.
- **Rejected:** A complete capture engine inside the injected script.
- **Reconsider with:** A browser API that exposes the same state through a
  typed protocol.

## ADR 004: Chromium CDP backend

- **Context:** Frames, network responses, browser contexts, and screenshots
  need browser-level control.
- **Decision:** Use Chromium through CDP behind `BrowserBackend`.
- **Consequences:** Browser-specific behavior stays in `pageknot-chromium`.
- **Rejected:** WebDriver as the canonical backend.
- **Reconsider with:** Equivalent lifecycle and observation evidence from
  another browser protocol.

## ADR 005: Narrow generated CDP client

- **Context:** Raw JSON at every call site obscures protocol drift.
- **Decision:** Generate selected domains from pinned official protocol JSON.
- **Consequences:** Input hashes and generated freshness are release gates.
- **Rejected:** A complete third-party CDP client and unrestricted generation.
- **Reconsider with:** A smaller maintained client that preserves the same
  revision and ownership guarantees.

## ADR 006: html5ever arena document

- **Context:** Transformation needs stable node identity and browser-compatible
  parsing.
- **Decision:** Parse into a PageKnot-owned arena built with html5ever.
- **Consequences:** Rewriting operates on stable IDs and validates structure
  after serialization.
- **Rejected:** String replacement as the document model.
- **Reconsider with:** A parser that improves compatibility without leaking its
  types into public APIs.

## ADR 007: Typed CSS parsing with preservation

- **Context:** CSS contains nested URLs and evolving syntax that must survive
  capture.
- **Decision:** Parse reference-bearing syntax while preserving untouched
  source ranges.
- **Consequences:** Rewrites are descending range edits and parser failures
  become structured outcomes.
- **Rejected:** Regular-expression CSS rewriting.
- **Reconsider with:** Corpus evidence that a full typed serializer preserves
  more author syntax and custom constructs.

## ADR 008: Safe-static HTML first

- **Context:** One portable file must reopen offline with bounded execution
  risk.
- **Decision:** Make self-contained safe-static HTML the initial artifact.
- **Consequences:** Captured scripts are sanitized and CSP blocks external
  connections.
- **Rejected:** Network replay archives as the default artifact.
- **Reconsider with:** A format that improves portability and keeps independent
  verification.

## ADR 009: Offline verification before commit

- **Context:** Encoding success does not prove self-containment.
- **Decision:** Reopen staged output with network access denied before success.
- **Consequences:** Verification is part of capture latency and terminal
  status.
- **Rejected:** Best-effort post-capture verification.
- **Reconsider with:** A static proof that covers browser loading behavior.

## ADR 010: Transactional output

- **Context:** Failure and cancellation must preserve the requested destination.
- **Decision:** Stage beside the destination and atomically commit verified
  bytes.
- **Consequences:** Conflict policy and filesystem behavior are explicit.
- **Rejected:** Streaming directly into the requested path.
- **Reconsider with:** A target storage API that supplies an equivalent commit
  primitive.

## ADR 011: Stable public DTO schema

- **Context:** Rust, CLI JSON, Node.js, and Python need semantic parity.
- **Decision:** Generate schemas and examples from canonical Rust records.
- **Consequences:** Field names, enum values, versions, and errors change
  through one model.
- **Rejected:** Hand-maintained binding DTOs.
- **Reconsider with:** A cross-language schema system that produces equal Rust
  ergonomics and compatibility checks.

## ADR 012: Thin napi-rs and PyO3 bindings

- **Context:** Native bindings should share capture behavior and cleanup.
- **Decision:** Map host objects to the Rust service through napi-rs and PyO3.
- **Consequences:** Binding methods release host runtime locks while awaiting
  Rust work.
- **Rejected:** Shelling out to the CLI from language packages.
- **Reconsider with:** A stable process protocol that improves installation and
  lifecycle evidence.

## ADR 013: Stable Rust toolchain

- **Context:** Contributors and release hosts need reproducible compiler
  behavior.
- **Decision:** Pin the stable toolchain in `rust-toolchain.toml`.
- **Consequences:** Toolchain upgrades are explicit compatibility changes.
- **Rejected:** Floating stable and nightly production builds.
- **Reconsider with:** A required feature whose risk is lower than its
  operational benefit.

## ADR 014: Bun collector tooling

- **Context:** The injected collector needs fast TypeScript tests and a
  deterministic browser bundle.
- **Decision:** Use Bun with a committed lockfile for collector development.
- **Consequences:** Bundle generation and freshness run in CI.
- **Rejected:** Adding a second JavaScript package manager.
- **Reconsider with:** Reproducibility or platform evidence against the pinned
  Bun workflow.

## ADR 015: Hermetic browser fixtures

- **Context:** Public websites drift and make release failures ambiguous.
- **Decision:** Serve controlled HTTP and HTTPS origins from
  `pageknot-test-support`.
- **Consequences:** Required browser gates run without the public internet.
- **Rejected:** Live-site tests as release gates.
- **Reconsider with:** A browser behavior that cannot be reproduced locally.

## ADR 016: SingleFile differential oracle

- **Context:** SingleFile has a broad behavior inventory and mature capture
  output.
- **Decision:** Compare fixture outcomes under the same Chromium environment.
- **Consequences:** DOM, frame, state, network, screenshot, size, duration, and
  memory metrics are recorded independently.
- **Rejected:** Byte equality between different artifact formats.
- **Reconsider with:** A stronger open reference corpus with comparable
  provenance.

## ADR 017: AGPL-compatible licensing

- **Context:** Design work included direct inspection of AGPL-licensed
  SingleFile source.
- **Decision:** License PageKnot under AGPL-3.0-or-later.
- **Consequences:** Source and distributed packages carry the same license
  posture.
- **Rejected:** A permissive initial license without qualified legal review.
- **Reconsider with:** Written legal guidance covering the inspected sources
  and intended distribution.
