# Architecture decisions

These decisions describe the current source. Reconsider a decision through a
change that updates implementation, tests, public contracts, and this record.

## Native service and frontends

**Context:** Four interfaces need one lifecycle, error model, and output
contract. Independent implementations would drift at browser and transaction
boundaries.

- The native CLI is the primary interactive and automation surface.
- The Rust `offprint` service owns capture behavior and lifecycle.
- CLI, Node.js, and Python map host input to canonical services and records.
- Public execution follows `CaptureRequest -> CaptureService -> CaptureJob -> CaptureReceipt`.

**Consequence:** Frontends map host input and output. Product behavior changes
in the service and canonical model first.

## Browser observation

**Context:** Rendered DOM, CSSOM, frames, shadow roots, canvas, and media state
exist inside the browser, while capture policy and file ownership belong on the
host.

- Chromium is the first browser adapter and uses CDP.
- `offprint-browser` owns provider-neutral ports.
- The injected TypeScript collector observes browser-only DOM, CSSOM, shadow,
  form, canvas, media, and frame state.
- Capture policy, resource failure policy, artifact encoding, and verification
  stay in Rust.
- CDP generation covers selected upstream types. The adapter also uses focused
  local request records for methods outside the generated subset.

**Consequence:** Collector code stays observational and bounded. Browser-specific
transport types remain behind the adapter boundary.

## Document and artifact

**Context:** A browser observation still contains external dependencies and
captured code. Encoding alone cannot prove safe-static or offline behavior.

- html5ever supplies browser-compatible parsing into an Offprint-owned arena.
- Typed CSS parsing discovers references, with source-preserving fallback for
  evolving syntax.
- Canonical output is safe-static Offprint HTML with exact owned restoration
  programs.
- Offline verification is the default. Static verification is an explicit
  weaker mode required by remote capture.
- File output stages beside the destination and commits after verification.
- Export formats derive from offline-verified HTML and own independent
  verifiers.

**Consequence:** Verification adds capture latency, and every representation
needs its own verifier and transaction evidence.

## Contracts and bindings

**Context:** Rust, JSON, Node.js, and Python need one record vocabulary while
retaining host-language naming and error ergonomics.

- `offprint-model` owns canonical serialized records and each record's field
  closure contract.
- JSON Schema, Node declarations, and Python stubs derive from those records.
- napi-rs and PyO3 bindings call the service rather than spawning the CLI.
- Product, public schema, artifact format, collector protocol, browser catalog,
  and CDP revisions evolve independently.

**Consequence:** Record changes require generation, binding parity, and
compatibility review across every host.

## Testing and release

**Context:** Public sites and browser versions drift, while process leaks,
partial output, and package-shape failures often appear outside unit tests.

- Hermetic browser fixtures own release behavior.
- SingleFile is a differential oracle rather than an architecture template.
- Rust toolchain and JavaScript and Python lockfiles are pinned.
- Release packages install and perform a real capture before the GitHub release
  is created.
- Original project code uses MIT. Distributed packages carry dependency
  notices and versioned source links under the recorded provenance.

**Consequence:** Hermetic fixtures gate releases. Live corpora remain
exploratory, and registry packages must complete capture after installation.
