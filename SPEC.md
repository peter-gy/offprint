# Offprint Product and Engineering Specification

| Field | Value |
| --- | --- |
| Status | Normative design baseline |
| Product target | Offprint 1.0 |
| Document version | 1 |
| Last revised | 2026-07-27 |
| Primary interface | Headless CLI and Rust service API |
| Initial artifact | Verified self-contained HTML |
| Initial browser backend | Chromium through the Chrome DevTools Protocol |

This document defines Offprint's user contract, public service API, command-line
interface, workspace architecture, capture model, security boundaries, language
binding strategy, validation system, implementation program, and release gates.

The terms MUST, SHOULD, and MAY describe requirements for Offprint 1.0. A
requirement can change through an accepted architecture decision record that
updates this specification and every affected public contract.

## 1. Executive decision

Offprint is a headless web capture engine with a native CLI and an importable
Rust service API. It loads a URL in Chromium, observes the rendered page,
collects the browser state and render-affecting resources, creates a
self-contained artifact, verifies the artifact under a denied-network policy,
and commits the requested output atomically.

The architecture has one public execution seam:

```text
CaptureRequest -> CaptureService -> CaptureJob -> CaptureReceipt
```

The CLI, Node.js binding, Python binding, and future hosts call this seam. They
do not invoke Chromium, parse HTML, resolve resources, or write artifacts
directly.

SingleFile supplies a behavior inventory and a differential reference. Its
module structure, option surface, and JavaScript execution model do not define
Offprint's architecture. Offprint starts from its own domain model and exposes a
smaller headless product:

- A capture is an explicit job.
- A job has a typed lifecycle.
- A rendered page becomes a typed internal captured page.
- Every discovered resource has a typed outcome.
- An artifact is written through a transaction.
- Verification is part of successful capture.
- CLI and SDK behavior share one implementation.
- Language bindings map stable records and service methods.

Offprint 1.0 ships the CLI, Rust API, Node.js binding, and Python binding from
one canonical model and service implementation.

## 2. Research basis

The design is informed by direct inspection of these sources:

- [Rewriting Bun in Rust](https://bun.com/blog/bun-in-rust), read in full on
  2026-07-27
- [Bun revision `4eb6f99`](https://github.com/oven-sh/bun/tree/4eb6f99c1afeae07a7445218c25b3b742b643f11)
- [SingleFile revision `7d556e7`](https://github.com/gildas-lormeau/SingleFile/tree/7d556e71fa3a700d833116f77d31a8b9c3669643)
- [agent-browser revision `3cc7022`](https://github.com/vercel-labs/agent-browser/tree/3cc7022271235694b5b5ce8aaea8bbfaa66e8cd5)
- [Codex revision `95637f7`](https://github.com/openai/codex/tree/95637f7056835fea66bdd0044414af480fc0fd74)

### 2.1 Lessons adopted from Bun's Rust rewrite

The Bun article describes a mechanical rewrite whose safety depended on
preparation, a language-independent test suite, compiler-driven work queues,
independent review, complete cross-platform CI, and continued fuzzing after the
merge. Offprint adopts the process controls that apply to a greenfield system:

1. Write the domain and lifetime contracts before implementation.
2. Keep behavioral fixtures independent of the implementation language.
3. Prove uncertain architecture with a small trial before scaling it.
4. Treat compiler errors and failing fixtures as explicit work queues.
5. Assign implementation and adversarial review to separate contexts for risky
   changes.
6. Reject stubs that exist to make compilation green.
7. Preserve safety checks and tests while fixing implementation failures.
8. Require every supported platform lane to execute the affected tests.
9. Use `Drop` and owned guards for browser processes, temporary stores,
   response streams, output transactions, and binding handles.
10. Fuzz parsers and serialized boundaries continuously.
11. Review source-language and target-language semantics where similar syntax
    has different behavior.
12. Separate merge confidence from release confidence.

The inspected Bun workspace contains 101 Cargo workspace packages and extensive
cross-language integration. Offprint crate boundaries follow ownership and
dependency direction, not a crate-count target.

Bun also encodes engineering knowledge in repository instructions, review
rules, command guards, generated schemas, and reproducible build commands.
Offprint adopts those repository-native controls at a scale appropriate to its
capture engine.

### 2.2 Lessons adapted for a greenfield product

Bun preserved architecture and behavior during a high-risk mechanical port.
Offprint has a different starting condition. SingleFile continues to exist
while Offprint is developed, so Offprint can use vertical product slices and
idiomatic Rust boundaries from the first commit.

The implementation program therefore uses:

- A fresh workspace
- Original Rust domain types
- A small observational TypeScript collector
- New fixtures written against observable browser behavior
- Differential comparisons against SingleFile
- Explicit parity decisions
- No source-file translation queue
- No temporary JavaScript-to-Rust compatibility layer

### 2.3 Lessons adopted from agent-browser and Codex

Offprint adopts these patterns:

- Direct, asynchronous Chromium control over a typed Chrome DevTools Protocol
  transport
- Real-browser end-to-end tests
- Purpose-specific Rust crates with directional dependencies
- Central workspace dependencies and lints
- A pinned Rust toolchain
- `just` as the documented contributor interface
- Explicit fixture groups for resource-heavy tests
- Cross-platform binary release tests
- Small public APIs that keep transport types private

Offprint uses Clap for its CLI, limits module size, and keeps browser actions out
of one large command module.

## 3. Problem statement

Saving a response body does not save a modern web page. The visible document
may depend on script-generated nodes, CSS Object Model rules, responsive image
selection, lazy loading, frames, shadow roots, canvas pixels, current form
state, fonts, browser layout, authentication, redirects, and time.

Existing page-saving systems commonly expose one or more of these problems:

- The file requests the network when reopened.
- Missing resources disappear silently.
- The CLI surface mirrors internal implementation switches.
- Cancellation leaves browser processes or partial files behind.
- A command-line wrapper contains the product logic.
- Programmatic callers must spawn the CLI and parse prose.
- JavaScript and Python clients develop different defaults.
- A successful exit status says little about artifact integrity.
- Browser and transformation responsibilities are intertwined.

Offprint gives users one inspectable result for one explicit capture request.
The result contains the artifact and evidence describing how it was produced.

## 4. Product goals

### 4.1 Primary goals

Offprint 1.0 MUST:

1. Capture a rendered page from Chromium in headless mode.
2. Preserve the visible document state required by the 1.0 fixture matrix.
3. Embed render-affecting resources into one HTML artifact.
4. Strip captured executable page scripts under the default policy.
5. Reopen the artifact with network access denied.
6. Record every discovered resource outcome.
7. Write explicit destinations transactionally.
8. Clean up browser and temporary resources on every terminal path.
9. Expose a high-ergonomy CLI.
10. Expose the same behavior through a Rust service API.
11. Keep public request, event, result, and error records binding-friendly.
12. Produce stable machine-readable output.
13. Run on Linux, macOS, and Windows.
14. Remain deterministic under fixed observations, policy, environment, and
    injected clock.
15. Bound time, bytes, nodes, frames, requests, redirects, and concurrency.

### 4.2 Secondary goals

The architecture SHOULD:

- Support Node.js and Python native bindings with thin adapters.
- Support alternate artifact encoders such as PDF and Markdown.
- Permit a managed Chromium build and a caller-owned remote CDP endpoint for
  static, unrestricted capture.
- Permit batch and crawl schedulers above the one-page capture service.
- Permit custom browser backends from Rust through an advanced API.
- Keep artifact format inspection independent of Chromium where possible.

### 4.3 Success measures

Offprint 1.0 is successful when:

- Every required browser fixture reopens with zero external network requests.
- Every required state invariant survives capture.
- Every failure path leaves the requested destination unchanged.
- Repeated capture jobs do not leak browser processes, file handles, temporary
  files, or unbounded resident memory.
- CLI, Rust API, and generated schemas agree on defaults and errors.
- Supported platform builds run their native boundary tests.
- A differential corpus shows explicit outcomes for each SingleFile behavior
  selected for 1.0.
- Release artifacts install and complete a real capture on clean machines.

## 5. Product boundaries

Offprint 1.0 captures individual rendered pages and schedules bounded batches
and breadth-first crawls as sets of independent captures.

The following capabilities have their own later milestones:

- Multi-page site archives
- WARC production
- Annotation and editing
- Cloud destination adapters
- Autosave
- Proof-of-existence providers
- Pixel-perfect replay of protected media
- Replay of arbitrary captured page scripts
- A general browser automation API

The capture engine can provide input to archival systems. The 1.0 artifact is a
rendered-page document with provenance, not a network-traffic archive.

## 6. Users and user stories

### 6.1 CLI users

- **OF-US-001:** As a researcher, I want to capture a rendered article into one
  file so that I can reopen the article after the source changes.
- **OF-US-002:** As a developer, I want one short command so that a capture does
  not require writing browser automation code.
- **OF-US-003:** As a developer, I want headless operation by default so that
  the same command works locally and in continuous integration.
- **OF-US-004:** As a developer, I want a headed diagnostic mode so that I can
  observe pages with unusual readiness behavior.
- **OF-US-005:** As an archivist, I want offline verification so that success
  means the saved file can render independently.
- **OF-US-006:** As an archivist, I want every missing resource listed so that
  fidelity gaps remain reviewable.
- **OF-US-007:** As a shell user, I want the artifact path on stdout so that I
  can pass it to another command.
- **OF-US-008:** As an automation author, I want progress on stderr so that
  stdout remains parseable.
- **OF-US-009:** As an automation author, I want one stable JSON object so that
  I can record digests, warnings, and timings.
- **OF-US-010:** As an operator, I want verified reruns to atomically replace
  their destination so repeated capture commands remain idempotent.
- **OF-US-011:** As an operator, I want a bounded timeout so that a page with
  permanent network activity cannot occupy a worker forever.
- **OF-US-012:** As an operator, I want Ctrl+C to stop browser and resource work
  so that interruption leaves no orphan process.
- **OF-US-013:** As a user with an authenticated page, I want cookies and
  headers loaded from protected files so that secrets stay out of process
  listings.
- **OF-US-014:** As a user capturing localhost, I want the standard network
  policy to permit an explicitly requested loopback origin.
- **OF-US-015:** As a server operator, I want a restrictive network profile so
  that untrusted URLs cannot reach private services.
- **OF-US-016:** As a user, I want `offprint doctor` to explain browser and
  configuration readiness before I run an expensive capture.

### 6.2 Rust SDK users

- **OF-US-017:** As a Rust developer, I want a `Offprint` handle so that I can
  reuse browser resources across captures.
- **OF-US-018:** As a Rust developer, I want a one-shot builder so that the
  common capture fits in a few lines.
- **OF-US-019:** As a service developer, I want a `CaptureJob` so that I can
  subscribe to progress, cancel work, and await the result.
- **OF-US-020:** As a service developer, I want an explicit `close` method so
  that shutdown can be awaited before my process exits.
- **OF-US-021:** As a test author, I want browser and clock dependencies
  injected behind narrow traits so that service behavior can be tested
  deterministically.
- **OF-US-022:** As an integrator, I want errors with stable codes and details
  so that I can implement recovery without matching prose.
- **OF-US-023:** As an integrator, I want plain request and result records so
  that I can serialize jobs across a queue.
- **OF-US-024:** As an integrator, I want byte and file output targets so that I
  can choose memory or transactional filesystem delivery.
- **OF-US-025:** As an integrator, I want a strict missing-resource policy so
  that my job can fail when archival completeness is required.

### 6.3 Binding users

- **OF-US-026:** As a Node.js developer, I want a Promise-based class API so
  that Offprint fits existing asynchronous applications.
- **OF-US-027:** As a Node.js developer, I want prebuilt native packages so that
  installation does not require a Rust toolchain.
- **OF-US-028:** As a Python developer, I want an asynchronous context manager
  so that browser resources close on scope exit.
- **OF-US-029:** As a Python developer, I want typed exceptions so that browser,
  policy, and verification failures are distinguishable.
- **OF-US-030:** As a binding user, I want the same option names and defaults as
  the CLI and Rust API so that examples transfer across environments.
- **OF-US-031:** As a binding user, I want typed progress events so that I can
  update a job UI without parsing logs.
- **OF-US-032:** As a binding maintainer, I want generated contract fixtures so
  that a language wrapper cannot drift from the Rust model silently.

### 6.4 Maintainers

- **OF-US-033:** As a maintainer, I want crate ownership boundaries so that
  browser, document, artifact, and interface changes can be reviewed
  independently.
- **OF-US-034:** As a maintainer, I want generated CDP and schema outputs
  checked for freshness so that build inputs remain reproducible.
- **OF-US-035:** As a maintainer, I want hermetic fixture servers so that
  browser tests do not depend on the public internet.
- **OF-US-036:** As a maintainer, I want high-level behavior tests shared across
  hosts so that CLI and SDK fixes protect the same contract.
- **OF-US-037:** As a maintainer, I want Miri and fuzz coverage on parsers and
  state machines so that malformed inputs are exercised continuously.
- **OF-US-038:** As a maintainer, I want review guidance beside the repository
  so that agents and humans apply the same lifecycle, security, and test rules.
- **OF-US-039:** As a release engineer, I want one version manifest so that
  binaries, crates, bindings, schemas, and artifact versions cannot drift
  unnoticed.
- **OF-US-040:** As a release engineer, I want install tests on every published
  package so that a successful build is not mistaken for a usable release.

## 7. Domain vocabulary

| Term | Meaning |
| --- | --- |
| Capture | One attempt to observe a URL and produce an artifact |
| Capture request | Validated input containing URL, environment, policy, limits, and output |
| Capture job | Cancellable asynchronous execution of a capture request |
| Capture receipt | Successful capture record containing artifact metadata, verification, warnings, and timings |
| Observation | Browser-originated facts collected before document transformation |
| Document | Parsed internal page model used during safe-static transformation |
| Frame graph | Parent and child relationships among captured documents |
| Resource reference | One HTML or CSS location that refers to a render-affecting resource |
| Resource record | Retrieval metadata, bytes, digest, provenance, and outcome for a resource |
| Captured page | Internal document, frame, and resource graphs ready for encoding |
| Artifact | User-visible encoded output |
| Artifact format | Encoded contract such as HTML, PDF, Markdown, ZIP, self-extracting HTML, or MHTML |
| Manifest | Versioned provenance and summary record embedded in the artifact |
| Policy | Explicit choices that change capture or output behavior |
| Limits | Resource ceilings that stop unbounded work |
| Collector | Small injected program that observes live browser state |
| Browser backend | Runtime implementation that navigates, evaluates, observes, and retrieves browser resources |
| Encoder | Component that converts a captured page into an artifact format |
| Verification report | Digest-bound evidence produced after verification succeeds |
| Service | Long-lived public object that owns one product capability |

These nouns MUST be used consistently in source, tests, schemas, CLI help,
diagnostics, and bindings.

## 8. Normative invariants

### 8.1 Capture invariants

1. The rendered browser state is the source of truth.
2. Every render-affecting reference receives a `ResourceOutcome`.
3. Collector messages are accepted after protocol negotiation.
4. Each frame belongs to one capture and has one stable `FrameId`.
5. A capture emits one terminal event.
6. Cancellation is idempotent.
7. A terminal capture cannot return to a running state.
8. Job events preserve stage order.
9. Event consumers cannot block capture execution.
10. Every acquired browser context, stream, temporary file, and process lease
    has one named owner.

### 8.2 Artifact invariants

1. The default safe-static artifact initiates zero external requests when
   reopened by the verifier.
2. The requested file path becomes visible after verification passes.
3. The artifact manifest and external result agree on counts and policy.
4. Artifact serialization is deterministic for fixed inputs and an injected
   clock.
5. A committed artifact is immutable from the perspective of its capture job.
6. Artifact format versions are independent from package versions.
7. The complete artifact digest is recorded outside the artifact.

### 8.3 API invariants

1. The CLI calls public Offprint services.
2. Binding crates call public Offprint services.
3. Core services do not read terminal globals.
4. Public request and result records avoid Rust lifetimes and generic type
   parameters.
5. Public enums serialize as tagged values with stable spellings.
6. Unknown schema fields are tolerated within a compatible major version.
7. Unknown enum formats fail with an explicit compatibility error.
8. User-reachable failures return typed errors.
9. Panics do not cross a language binding boundary.
10. Rust, Node.js, Python, and CLI defaults are generated or tested from one
    canonical source.

## 9. Offprint 1.0 capability contract

### 9.1 Required capture state

Offprint 1.0 captures:

- Final rendered HTML
- Doctype, title, base URL, and document encoding
- Current form control values and selections
- Open details state
- Current responsive image selection
- Canvas pixels
- Media poster or current-frame fallback
- Open shadow roots
- Closed shadow roots observed by the document-start hook
- Adopted stylesheets
- CSSOM stylesheet contents
- Same-origin frames
- Cross-origin out-of-process frames
- `srcdoc` frames
- Browser-selected fonts and stylesheets
- Render-affecting images, fonts, media, SVG, and CSS resources
- Redirect and response provenance
- Browser environment and capture policy

### 9.2 Required artifact behavior

The default artifact:

- Is one HTML file
- Contains embedded render-affecting resources
- Contains a restrictive content security policy
- Contains a versioned Offprint manifest
- Removes executable captured page scripts
- Preserves non-executable structured metadata
- Reopens from a local file URL
- Reaches a stable state with network access denied
- Reports every permitted fidelity gap

### 9.3 Policy tiers

Offprint exposes three named policy profiles:

| Profile | Intended use | Missing resources | Verification | Network |
| --- | --- | --- | --- | --- |
| `default` | Interactive CLI capture | Warn | Offline | Standard |
| `strict` | Archival and CI | Fail | Offline | Standard |
| `server` | Multi-tenant service | Fail | Offline | Public-only |

Named profiles expand into ordinary typed options. Users may override individual
fields.

## 10. System architecture

```text
CLI          Node.js          Python          Rust caller
 │              │                │                │
 └──────────────┴────────────────┴────────────────┘
                         Offprint
             ┌──────────────┼──────────────┐
             │              │              │
      CaptureService  ArtifactService  BrowserService
             │              │              │
             └──────────────┼──────────────┘
                    Capture pipeline
             ┌──────────────┼──────────────┐
             │              │              │
       BrowserBackend  Document pipeline  Artifact pipeline
             │              │              │
       Chromium + CDP  HTML/CSS/resources  encode/verify/commit
             │
       TypeScript collector
```

### 10.1 Public application boundary

`Offprint` constructs and owns service dependencies. It is a cloneable handle
over shared runtime state. Cloning the handle does not launch another browser.

The public services are:

- `CaptureService`
- `ArtifactService`
- `BrowserService`

These services are explicit classes in Node.js and Python and explicit structs
in Rust.

### 10.2 Internal ownership

`CaptureService` owns job registration, cancellation, events, and the terminal
result. `pipeline::run_capture_job` receives one validated request and creates
stage-owned values for browser, document, resource, verification, and commit
work.

Internal transformation code uses pure functions or focused stateful types.
The implementation should not create a `Service` type for a pure operation
merely to match an architectural label.

### 10.3 One-page boundary

`CaptureService` captures one URL per request. Batch and crawl systems schedule
several independent requests and aggregate results. This keeps retry,
concurrency, persistence, and partial failure policy outside page capture.

## 11. Rust workspace

### 11.1 Proposed tree

```text
offprint/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── justfile
├── AGENTS.md
├── REVIEW.md
├── README.md
├── SPEC.md
├── crates/
│   ├── offprint/
│   ├── offprint-model/
│   ├── offprint-protocol/
│   ├── offprint-browser/
│   ├── offprint-chromium/
│   ├── offprint-document/
│   ├── offprint-capture/
│   ├── offprint-artifact/
│   ├── offprint-html/
│   ├── offprint-transform/
│   ├── offprint-export/
│   ├── offprint-cli/
│   └── offprint-test-support/
├── bindings/
│   ├── node/
│   └── python/
├── collector/
│   ├── package.json
│   ├── bun.lock
│   ├── src/
│   ├── tests/
│   └── dist/
├── schemas/
├── fixtures/
├── fuzz/
├── benches/
├── xtask/
└── .github/
    └── workflows/
```

### 11.2 Crate ownership

#### `offprint-model`

Owns public and shared domain records:

- Request records
- Policies
- Limits
- Events
- Results
- Error records
- Manifest records
- Schema generation
- Identifiers and version types

The crate performs no browser, terminal, or filesystem I/O.

#### `offprint-protocol`

Owns the host-to-collector protocol:

- Handshake
- Capabilities
- Chunk envelopes
- Collector commands and diagnostics
- Protocol version negotiation
- Shared Rust and TypeScript fixtures

The collector protocol is internal and versioned independently from the public
SDK schema.

#### `offprint-browser`

Owns browser ports and provider-neutral observation records:

- `BrowserBackend`
- `BrowserLease`
- `BrowserContext`
- `PageSession`
- `FrameObservation`
- Navigation and readiness evidence
- Resource body streams
- Browser capabilities

The crate has no collector-protocol or CDP dependency.

#### `offprint-chromium`

Owns:

- Chromium discovery
- Managed-browser provisioning
- Process launch and teardown
- CDP code generation
- WebSocket transport
- Target and session routing
- Frame attachment
- Collector injection
- Browser resource access
- Network-denied verification contexts

#### `offprint-document`

Owns:

- Arena DOM
- Frame graph
- Resource graph
- HTML parsing
- CSS parsing and rewriting
- Live-state application
- Shadow tree materialization
- Frame materialization
- Script sanitization
- Deterministic serialization primitives

#### `offprint-capture`

Owns:

- Capture stage state machine
- Limits and budgets
- Cancellation propagation
- Validated capture requests
- Content-addressed temporary storage

#### `offprint-artifact`

Owns:

- Bounded memory output
- Single-file staging and commit
- Multi-output staging, recovery, and commit
- Conflict policy application
- Output digests and transaction records

#### `offprint-html`

Owns:

- HTML artifact encoder
- Data URL encoding
- Embedded manifest
- Content security policy
- Static HTML verifier
- HTML artifact inspector

#### `offprint-transform`

Owns the shared safe-static transformation boundary:

- Rendering freeze
- Active-content sanitization
- Structural repair
- HTML encoding through the format boundary

The native capture pipeline calls this crate after browser state and resources
have been collected.

#### `offprint-export`

Owns alternate artifact formats:

- PDF format verification
- Markdown and relative asset encoding
- Deterministic ZIP encoding
- Self-extracting HTML encoding
- MHTML encoding
- Format-specific structural verification

#### `offprint`

Owns the public service API:

- `Offprint`
- `OffprintBuilder`
- `CaptureService`
- `Capture`
- `CaptureJob`
- `ArtifactService`
- `BrowserService`
- Dependency composition
- One-shot convenience functions

#### `offprint-cli`

Owns:

- Clap command definitions
- Configuration discovery
- Secret-file loading
- Human rendering
- JSON rendering
- Exit status mapping
- Signal handling
- Shell completion

#### `offprint-test-support`

Owns:

- Fixture server
- Multiple origins
- HTTPS test authority
- Explicit fixture-to-test ownership

Workspace test targets resolve it through path development dependencies.
Registry packages contain the runtime dependency graph, while
`offprint-test-support` remains workspace-local.

### 11.3 Dependency direction

```text
offprint-cli, bindings/node, bindings/python
  -> offprint

offprint
  -> artifact, browser, capture, chromium, document, export, html,
     model, protocol, transform

chromium
  -> browser, model, protocol
browser
  -> model
transform, export
  -> document, html, model
html
  -> document, model
artifact, capture, document, protocol
  -> model
```

Cargo metadata tests MUST reject dependency cycles and forbidden upward edges.

### 11.4 Workspace policy

- Rust edition 2024
- Stable pinned Rust toolchain
- Explicit minimum supported Rust version after 1.0
- Resolver 3
- Central workspace dependencies
- Central workspace lints
- Complete registry metadata for crates published from the workspace
- `publish = false` for bindings, benchmarks, and maintenance binaries released
  through another channel or kept local
- Supported public Rust APIs limited to `offprint`, `offprint-model`, and
  `offprint-cli` for 1.0
- Artifact, browser, capture, Chromium, document, export, HTML, protocol, and
  transform crates published as registry dependency units for `offprint`
- `offprint-test-support` kept workspace-local with `publish = false`
- No generic `core`, `common`, `shared`, `helpers`, or `utils` crate
- First-party production crates use `unsafe_code = "forbid"`
- Binding crates contain documented boundary exceptions when required
- Release profiles retain symbol files for crash diagnosis
- CLI release uses thin link-time optimization
- Library and binding builds retain unwind behavior

## 12. Public service API

### 12.1 `Offprint`

`Offprint` owns shared services and lazily acquires browser resources.

```rust
pub struct Offprint {
    state: Arc<RuntimeState>,
}

impl Offprint {
    pub fn builder() -> OffprintBuilder;
    pub fn captures(&self) -> CaptureService;
    pub fn artifacts(&self) -> ArtifactService;
    pub fn browsers(&self) -> BrowserService;
    pub fn capture(&self, url: impl AsRef<str>) -> Result<Capture>;
    pub async fn close(&self) -> Result<()>;
}
```

Contract:

- `build` validates configuration and initializes local state.
- `OffprintBuilder` configures profiles, browser selection, installation policy,
  network policy, concurrency, cache paths, and diagnostics.
- Browser launch occurs when a browser capability is first requested.
- `close` cancels active jobs, waits for cleanup, and is idempotent.
- Operations other than `close` return `offprint.runtime.closed` after
  successful close.
- `Drop` requests best-effort cleanup.
- Language binding finalizers provide best-effort cleanup.
- Explicit `close` is the supported shutdown contract.

### 12.2 `CaptureService`

```rust
#[derive(Clone)]
pub struct CaptureService {
    state: Arc<RuntimeState>,
}

impl CaptureService {
    pub async fn start(&self, request: CaptureRequest) -> Result<CaptureJob>;
    pub async fn batch(&self, request: BatchRequest) -> Result<BatchResult>;
    pub async fn crawl(&self, request: CrawlRequest) -> Result<CrawlResult>;
}
```

`start` performs complete request validation before registering browser work.
It returns after the job has a stable ID and event channel.

`batch` runs independent requests with bounded concurrency and one terminal
outcome per job. `crawl` follows a deterministic breadth-first URL frontier.
Both operations can persist atomic resume state after each terminal outcome.

### 12.3 `Capture`

The one-shot API is optimized for the common path:

```rust
let result = offprint
    .capture("https://example.com")?
    .save("example.html")
    .await?;
```

Builder methods include:

```rust
Capture::output(path)
Capture::profile(name)
Capture::timeout(duration)
Capture::wait_until(mode)
Capture::delay(duration)
Capture::viewport(viewport)
Capture::strict()
Capture::headed(bool)
Capture::save(path)
Capture::bytes(max_bytes)
Capture::start()
Capture::run()
```

Builder methods consume or return `Self`. They use domain enums in place of
boolean parameters where two states need names.

- `save(path)` sets a file target, starts the job, and awaits its result.
- `bytes(max_bytes)` sets a bounded memory target, starts the job, and awaits
  its result.
- `start()` returns a `CaptureJob`.
- `run()` starts the configured job and awaits its result.

### 12.4 `CaptureJob`

```rust
pub struct CaptureJob {
    id: CaptureId,
    state: Arc<JobState>,
}

impl CaptureJob {
    pub fn id(&self) -> &CaptureId;
    pub fn status(&self) -> CaptureStatus;
    pub fn events(&self) -> impl Stream<Item = CaptureEvent>;
    pub fn cancel(&self);
    pub async fn result(&self) -> Result<CaptureReceipt>;
}
```

Contract:

- `cancel` is non-blocking and idempotent.
- `wait` returns after cleanup and output rollback or commit.
- Dropping one cloned job handle does not cancel the job.
- `Offprint::close` cancels jobs still owned by the runtime.
- Every event subscriber receives events from its subscription point.
- Slow subscribers receive coalesced progress records.
- Warning and terminal events are retained.
- One terminal result is stored until all job handles are dropped.

### 12.5 `ArtifactService`

```rust
impl ArtifactService {
    pub async fn inspect(&self, input: ArtifactSource) -> Result<ArtifactManifest>;
    pub async fn verify(
        &self,
        input: ArtifactSource,
        policy: VerificationMode,
    ) -> Result<VerificationReport>;
    pub async fn export(
        &self,
        input: ArtifactSource,
        request: ExportRequest,
    ) -> Result<ExportResult>;
    pub async fn export_capture(
        &self,
        capture: &CaptureReceipt,
        request: ExportRequest,
    ) -> Result<ExportResult>;
    pub async fn verify_format(
        &self,
        path: PortablePath,
        kind: ArtifactFormat,
    ) -> Result<FormatVerification>;
}
```

Static inspection can run without Chromium. Offline verification acquires a
browser context through `BrowserService`. `export` derives each requested
format from one offline-verified HTML artifact and verifies every format before
commit. `export_capture` revalidates the safe-static HTML and reuses matching
offline evidence from a completed `CaptureReceipt`. `verify_format` applies the
format-specific verifier to an exported path.

### 12.6 `BrowserService`

```rust
impl BrowserService {
    pub async fn ensure(&self) -> Result<BrowserInfo>;
    pub async fn install(&self, request: BrowserInstallRequest) -> Result<BrowserOperationResult>;
    pub async fn list(&self) -> Result<BrowserOperationResult>;
    pub async fn remove(&self, revision: &str, force: bool) -> Result<BrowserOperationResult>;
    pub async fn doctor(&self) -> BrowserDoctorReport;
    pub async fn close_idle(&self) -> Result<()>;
}
```

Managed browser installation verifies the published archive digest before
extraction and commits the browser directory atomically.

The default installation policy permits managed provisioning. `ensure`
discovers an installed managed browser or a compatible system browser, then
downloads and verifies the pinned managed build when discovery finds neither.
`BrowserChannel::System` and an explicit executable keep local browser
selection under caller control. Applications can select
`BrowserInstallationPolicy::Explicit` when capture must not populate the
managed browser cache.

Release targets with managed provisioning are Linux x86-64 with glibc, macOS
x86-64 and arm64, and Windows x86-64. Other builds use a compatible
caller-selected system browser.

A remote CDP endpoint supports capture requests that explicitly select
`NetworkPolicy::Unrestricted` and `VerificationMode::Static`. The caller owns
endpoint trust, browser lifecycle, network controls, and browser-side state.
`BrowserDoctorReport` marks restricted network policies and offline
verification unavailable for remote endpoints.

`BrowserDoctorReport` records the selected browser, every discovered candidate,
compatibility decisions, managed revision and cache state, collector handshake
capabilities, output transaction readiness, effective configuration provenance,
and network policy. Human and JSON renderers use the common redaction layer.

### 12.7 Public records

Public records use owned binding-friendly values:

- `String`
- `Url`
- `PortablePath`, represented as a UTF-8 string
- Integer byte counts
- `Duration` represented as integer milliseconds in schemas
- Tagged enums
- Vectors and maps with string keys
- Optional fields for genuinely absent values

Internal traits, streams, browser sessions, arena nodes, parser types, and
temporary store handles stay outside public records.

The CLI validates native paths as UTF-8 before browser work. This keeps path
values identical across Rust, JSON, Node.js, and Python. An invalid path returns
`offprint.input.path_encoding`.

## 13. Language bindings

### 13.1 Binding architecture

```text
offprint-model  -> canonical DTOs and JSON Schema
offprint        -> canonical service behavior
       │
       ├── offprint-node   -> napi-rs adapter
       └── offprint-python -> PyO3 adapter
```

The Node.js binding uses [napi-rs](https://napi.rs/). Node-API gives the native
addon an ABI compatibility boundary, and napi-rs supports generated TypeScript
declarations and per-platform native packages.

The Python binding uses [PyO3](https://pyo3.rs/) and
[Maturin](https://www.maturin.rs/) with an `abi3` release strategy when the
selected async integration supports the target Python matrix.

Binding crates contain mapping code. Capture policy and behavior remain in
`offprint`.

### 13.2 Canonical schema

`offprint-model` is the canonical definition of:

- `CaptureRequest`
- `ContentPolicy`
- `CaptureLimits`
- `CaptureEvent`
- `CaptureReceipt`
- `BatchRequest`
- `BatchResult`
- `CrawlRequest`
- `CrawlResult`
- `OffprintError`
- `ArtifactManifest`
- `ExportRequest`
- `ExportResult`
- `FormatVerification`
- `VerificationReport`
- `BrowserDoctorReport`
- `BrowserOperationResult`

An `xtask` generates:

- JSON Schema
- Example JSON fixtures
- TypeScript fixture types
- Python fixture models
- CLI JSON documentation

Generated declarations from napi-rs and Python stubs are compared with the
canonical fixtures. A binding release fails when the field names, enum values,
optional behavior, or defaults drift.

### 13.3 Node.js API

```ts
const offprint = new Offprint(options?)

await offprint.capture(url, options?)
await offprint.captures.start(request)
await offprint.artifacts.inspect(path)
await offprint.artifacts.verify(path, options?)
await offprint.browsers.ensure()
await offprint.close()
```

`CaptureJob` exposes:

```ts
job.id
job.status
job.events()
job.cancel()
job.result()
```

`events()` returns an asynchronous iterator. The adapter receives events through
a bounded channel and schedules delivery on the JavaScript thread. Callback
latency cannot stall the Rust capture pipeline.

Errors extend `OffprintError` and expose:

```ts
error.code
error.stage
error.retryable
error.details
error.diagnosticsPath
```

### 13.4 Python API

```python
offprint = Offprint(options=None)

await offprint.capture(url, **options)
await offprint.captures.start(request)
await offprint.artifacts.inspect(path)
await offprint.artifacts.verify(path, **options)
await offprint.browsers.ensure()
await offprint.close()
```

`Offprint` implements an asynchronous context manager. `CaptureJob.events()` is
an asynchronous iterator.

Python exceptions share a base `OffprintError` and expose the same structured
fields as Node.js.

### 13.5 Async runtime ownership

- The Rust library runs inside the caller's Tokio runtime.
- The CLI constructs one Tokio runtime.
- Node.js integrates through napi-rs asynchronous tasks.
- Python integrates through the selected PyO3 async runtime adapter.
- Binding methods release language runtime locks while awaiting Rust work.
- Rust callbacks enter the host runtime through the binding's supported
  scheduling primitive.
- No binding starts a Tokio runtime inside an existing Tokio runtime.
- `Offprint::close` waits for job and browser cleanup in every host.

### 13.6 Panic boundary

Public services return errors for user-controlled input and external failures.
Binding entry points also catch unexpected Rust panics, convert them to
`offprint.internal.panic`, record a sanitized diagnostic, and keep unwind state
inside the native boundary.

The panic conversion is a containment measure. A reachable panic remains a
release-blocking defect.

### 13.7 Versioning

- All first-party packages share a product SemVer version.
- Artifact format, public JSON schema, collector protocol, and CDP pin have
  independent versions.
- Minor releases may add optional record fields.
- Enum additions require bindings to support unknown-value diagnostics.
- Field removal, meaning changes, and default changes require a product major
  version.
- Bindings declare the exact compatible native library range.

## 14. Command-line interface

### 14.1 Command tree

```text
offprint capture <URL> [OPTIONS]
offprint artifact export <ARTIFACT> [OPTIONS]
offprint batch <MANIFEST|-> [OPTIONS]
offprint crawl <URL> [OPTIONS]
offprint artifact verify <ARTIFACT> [OPTIONS]
offprint artifact inspect <ARTIFACT> [OPTIONS]
offprint doctor [OPTIONS]
offprint browser install [OPTIONS]
offprint browser list [OPTIONS]
offprint browser remove <REVISION> [OPTIONS]
offprint completion <SHELL>
```

The canonical verb is `capture`.

`browser remove` refuses to remove the selected or active revision. `--force`
allows removal of the selected idle revision after another compatible browser
has been resolved. The command never removes an active browser lease.

### 14.2 `offprint capture`

Synopsis:

```text
offprint capture <URL>
  -o, --output <PATH|->
  [--on-exists <fail|replace|uniquify>]
  [--profile <NAME>]
  [--config <PATH>]
  [--browser-path <PATH> | --cdp-url <URL>]
  [--headed]
  [--viewport <WIDTHxHEIGHT>]
  [--locale <LOCALE>]
  [--timezone <ZONE>]
  [--color-scheme <light|dark>]
  [--timeout <DURATION>]
  [--wait-until <render-idle|network-idle|load|dom-content-loaded>]
  [--delay <DURATION>]
  [--missing-resources <warn|fail>]
  [--scope <page|selection>]
  [--selector <CSS>]
  [--remove-unused-css]
  [--remove-unused-fonts]
  [--remove-hidden-elements]
  [--verification <static|offline>]
  [--headers <PATH>]
  [--cookies <PATH>]
  [--network-policy <standard|server|unrestricted>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
  [--diagnostics <DIR>]
```

#### Arguments and options

| Input | Contract |
| --- | --- |
| `URL` | Absolute `http`, `https`, or explicitly permitted `file` URL |
| `--output` | Required capture artifact path, or `-` for artifact bytes on stdout |
| `--on-exists` | Destination conflict behavior. Defaults to `fail` |
| `--profile` | Named configuration profile |
| `--config` | Explicit TOML configuration |
| `--browser-path` | Local Chrome or Chromium executable |
| `--cdp-url` | Caller-owned endpoint for static, unrestricted capture |
| `--headed` | Display the browser window for diagnosis |
| `--viewport` | CSS viewport width and height |
| `--locale` | Browser locale |
| `--timezone` | Browser timezone |
| `--color-scheme` | Emulated color preference |
| `--timeout` | Total capture deadline |
| `--wait-until` | Readiness condition |
| `--delay` | Additional delay after observable readiness |
| `--missing-resources` | Whether unresolved resources warn or fail |
| `--scope` | Complete page or active top-level selection |
| `--selector` | First top-level document element matched by `document.querySelector` |
| `--remove-unused-css` | Remove CSS rules that cannot match the captured state |
| `--remove-unused-fonts` | Remove font faces unused by the captured state |
| `--remove-hidden-elements` | Remove elements with computed `display: none` |
| `--verification` | Required verification mode |
| `--headers` | Protected JSON file containing request headers |
| `--cookies` | Protected JSON file containing browser cookies |
| `--network-policy` | Address and redirect policy |
| `--json` | Emit one `CaptureReceipt` |
| `--quiet` | Suppress non-error human diagnostics |
| `--color` | Human diagnostic color behavior |
| `--diagnostics` | Directory for sanitized failure artifacts |

Validation occurs before browser launch where the needed information is
available.

Incompatible combinations:

- `--browser-path` with `--cdp-url`
- `--json` with `--output -`
- `--selector` with `--scope`
- Header or cookie input from stdin with `--output -`
- `file:` URL outside an allowed root
- `--verification offline` when the selected browser lacks the required capability
- Remote CDP with an effective network policy other than `unrestricted`
- Remote CDP with an effective verification policy other than `static`

Selector capture preserves the document head and the matched element's ancestor
chain. It prunes sibling content before frame mapping and resource
materialization. Invalid selector syntax returns `offprint.selector.invalid`.
A valid selector with no match returns `offprint.selector.not_found`.

`capture` produces the canonical Offprint HTML artifact. PDF and the other
artifact formats are derived through `export`.

### 14.3 `offprint artifact export`

Synopsis:

```text
offprint artifact export <ARTIFACT|->
  --output <DIR>
  --format <pdf|markdown|zip|self-extracting-html|mhtml>...
  [--base-name <NAME>]
  [--landscape]
  [--prefer-css-page-size]
  [--no-front-matter]
  [--on-exists <fail|replace|uniquify>]
  [--config <PATH>]
  [--browser-path <PATH>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`export` reopens one Offprint HTML artifact with network access denied, then
derives and verifies each requested format before committing it. Repeat
`--format` or pass a comma-separated list. Human output writes one committed
entrypoint per line. `--json` emits one `ExportResult`.

PDF options require the `pdf` format. `--no-front-matter` requires the
`markdown` format. The conflict policy defaults to `fail` and applies
independently to each output.

### 14.4 `offprint batch`

Synopsis:

```text
offprint batch <MANIFEST|->
  [--config <PATH>]
  [--browser-path <PATH> | --cdp-url <URL>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`batch` reads one versioned `BatchRequest` from a bounded JSON file or standard
input. The request owns job IDs, complete capture requests, concurrency, and
optional resume state. Each job reaches an independent terminal outcome.

Human output reports succeeded, failed, and resumed counts. `--json` emits one
`BatchResult`. A result with failed jobs is still written before the command
returns status `1`.

When `--cdp-url` selects a remote browser, every capture request in the batch
must select `NetworkPolicy::Unrestricted` and `VerificationMode::Static`.

### 14.5 `offprint crawl`

Synopsis:

```text
offprint crawl <URL>
  --output <DIR>
  [--profile <NAME>]
  [--config <PATH>]
  [--max-pages <COUNT>]
  [--max-depth <COUNT>]
  [--concurrency <COUNT>]
  [--resume <PATH>]
  [--retry-failed]
  [--allow-cross-origin]
  [--browser-path <PATH>]
  [--headed]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`crawl` captures a deterministic breadth-first frontier rooted at `URL`. It
keeps navigation on the seed origin unless `--allow-cross-origin` is set and
writes one offline-verified HTML artifact per page. Page and depth limits are
enforced before new work is scheduled.

`--resume` persists atomic scheduler state. `--retry-failed` requires a resume
path and schedules prior failed outcomes again. Human output reports page
counts. `--json` emits one `CrawlResult`. A result with failed pages is written
before the command returns status `1`.

### 14.6 `offprint artifact verify`

Synopsis:

```text
offprint artifact verify <ARTIFACT|->
  [--format <pdf|markdown|zip|self-extracting-html|mhtml>]
  [--verification <static|offline>]
  [--config <PATH>]
  [--browser-path <PATH>]
  [--timeout <DURATION>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
  [--diagnostics <DIR>]
```

`verify` validates an existing Offprint artifact. Its default level is
`offline`.

`static` checks:

- Artifact readability and supported format version
- Manifest schema and digest
- CSP presence and policy consistency
- Controlled resource references
- Embedded resource syntax and digests
- Forbidden active content

`offline` performs every static check, then opens the artifact in a fresh
network-denied browser context. It records attempted requests, uncaught page
errors, frame load outcomes, and readiness. Any network attempt fails safe-static
verification even when Chromium blocks the request.

`ARTIFACT` accepts a file path or `-`. Standard input is copied into bounded
temporary storage before verification. `--browser-path` applies only to
offline verification.

`--format` applies the verifier for an exported artifact and requires a
filesystem path. HTML verification options apply when `--format` is absent.

Human success output names the artifact, verification mode or format, digest,
byte count, and zero-network result. `--json` emits one `VerificationReport`
for HTML or one `FormatVerification` for an exported format.
Verification failure returns exit status `3`. An unreadable input or browser
runtime failure returns `1`.

### 14.7 `offprint artifact inspect`

Synopsis:

```text
offprint artifact inspect <ARTIFACT|->
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`inspect` parses the embedded manifest without launching Chromium. It validates
the artifact envelope before rendering the manifest.

- Human output shows format version, redacted source, capture time, generator,
  browser, environment, policy digest, resource outcomes, warnings, and
  verification record.
- `--json` emits exactly one versioned `ArtifactManifest`.
- `-` reads a bounded artifact from stdin.
- Secret URL components remain redacted in every renderer.
- A missing, malformed, unsupported, or inconsistent manifest returns `1`.

### 14.8 `offprint doctor`

Synopsis:

```text
offprint doctor
  [--config <PATH>]
  [--browser-path <PATH> | --cdp-url <URL>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`doctor` is read-only. It resolves configuration, discovers browser candidates,
checks the managed cache, validates browser compatibility, tests collector
protocol compatibility, and checks whether the current directory can support
an atomic output transaction. It does not install a browser or launch a capture.

Human output gives one selected browser or one recovery command. `--json` emits
one `BrowserDoctorReport`, including the origin of each effective configuration
value. The report excludes secrets.

Exit status `0` means the default capture path is ready. Status `1` means a
runtime capability is unavailable. Invalid configuration returns `2`.

### 14.9 `offprint browser`

Install:

```text
offprint browser install
  [--revision <REVISION>]
  [--cache-dir <DIR>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

The default revision comes from Offprint's version manifest. An explicit
revision must exist in the trusted browser catalog with a known archive digest.
The command downloads into bounded temporary storage, verifies the digest,
validates the archive, extracts into staging, probes the executable, and commits
the managed directory atomically. Concurrent installers share a cache lock and
reuse a valid completed installation.

List:

```text
offprint browser list
  [--cache-dir <DIR>]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`list` reports managed revisions and discovered system candidates. Each record
contains product, version, executable path, source, compatibility, selection
priority, and active lease count.

Remove:

```text
offprint browser remove <REVISION>
  [--cache-dir <DIR>]
  [--force]
  [--json]
  [--quiet]
  [--color <auto|always|never>]
```

`remove` accepts a managed revision identifier, never an arbitrary filesystem
path. It resolves and validates the exact cache entry before mutation. An active
lease always blocks removal. The selected idle revision requires `--force` and
another compatible browser. Removal renames the revision into a cache-local
trash entry before deleting it so discovery never observes a partial tree.

Browser commands never prompt. Human success output names the installed,
listed, or removed revision. `--json` emits one versioned browser operation
result.

### 14.10 `offprint completion`

Synopsis:

```text
offprint completion <bash|elvish|fish|powershell|zsh>
```

The command writes the generated completion script to stdout and diagnostics to
stderr. It does not install or modify shell files. The script matches the
command tree in the running binary.

### 14.11 Default output

When `--output` is absent:

1. Offprint captures the title.
2. It normalizes the title into a portable filename.
3. It falls back to the final hostname when the title is empty.
4. It appends `.html`.
5. It atomically replaces that path after verification succeeds.

Explicit output paths also use `ConflictPolicy::Replace`. Failed and cancelled
captures leave the current destination unchanged.

### 14.12 stdout and stderr

Normal successful capture:

- stdout contains the committed artifact path followed by one newline.
- stderr contains progress and warnings.

`--quiet`:

- stdout still contains the artifact path.
- stderr contains errors.

`--json`:

- stdout contains exactly one JSON object followed by one newline.
- stderr contains human progress unless `--quiet` is set.

`--output -`:

- Offprint captures and verifies into temporary storage.
- stdout receives the complete artifact after verification.
- stderr receives diagnostics.
- A stdout write failure returns a runtime failure status.

### 14.13 Human diagnostics

Human output names:

- Current stage
- Source URL
- Browser selection
- Readiness result
- Resource counts
- Warnings
- Verification result
- Final path

Interactive progress uses bounded updates. Non-TTY stderr receives stable line
records.

### 14.14 Exit statuses

| Code | Meaning |
| ---: | --- |
| `0` | Requested operation succeeded |
| `1` | Capture, browser, output, or runtime failure |
| `2` | Invalid arguments or configuration |
| `3` | Artifact verification failed |
| `130` | Interrupted |

Warnings permitted by policy return `0`.

### 14.15 Configuration

Configuration precedence:

```text
CLI flags
OFFPRINT_* environment variables
explicit --config file
selected profile
user configuration
built-in defaults
```

Offprint does not automatically load project-directory configuration. This
prevents an untrusted checkout from changing browser or network policy.

Platform user configuration paths are resolved through native platform
conventions. `offprint doctor --json` reports the effective configuration with
secrets redacted.

Secrets are referenced by file or environment variable. Human and JSON
diagnostics never include header values, cookie values, credentials, or full
signed query strings.

### 14.16 Machine result

Example:

```json
{
  "schemaVersion": 2,
  "captureId": "cap_01J...",
  "source": {
    "requestedUrl": "https://example.com",
    "finalUrl": "https://example.com/"
  },
  "artifact": {
    "kind": "file",
    "path": "example.html",
    "bytes": 43821,
    "sha256": "..."
  },
  "verification": {
    "level": "offline",
    "networkRequests": 0
  },
  "resources": {
    "discovered": 14,
    "embedded": 14,
    "external": 0,
    "omitted": 0,
    "failed": 0
  },
  "warnings": [],
  "timings": {
    "totalMs": 1834,
    "navigationMs": 412,
    "settleMs": 301,
    "collectionMs": 245,
    "encodingMs": 121,
    "verificationMs": 407
  }
}
```

## 15. Domain model

### 15.1 Identifiers

```rust
pub struct CaptureId(String);
pub struct FrameId(u64);
pub struct NodeId(u32);
pub struct ResourceId(u32);
pub struct ContentDigest([u8; 32]);
```

String representations include a type prefix where users or logs see the ID.

### 15.2 Capture request

```rust
pub struct CaptureRequest {
    pub url: Url,
    pub output: CaptureOutput,
    pub browser: BrowserSpec,
    pub environment: BrowserEnvironment,
    pub readiness: ReadinessPolicy,
    pub content: ContentPolicy,
    pub network: NetworkPolicy,
    pub limits: CaptureLimits,
    pub verification: VerificationMode,
    pub diagnostics: DiagnosticsPolicy,
}
```

#### Capture output

```rust
pub enum CaptureOutput {
    File {
        path: PortablePath,
        conflict: ConflictPolicy,
    },
    Memory {
        max_bytes: u64,
    },
}
```

`CaptureOutput` selects bounded bytes or a transactional file for the canonical
HTML capture. Alternate artifact formats are derived through `ArtifactService`.

#### Capture receipt

```rust
pub struct CaptureReceipt {
    pub schema_version: u32,
    pub capture_id: CaptureId,
    pub source: SourceSummary,
    pub artifact: CaptureArtifact,
    pub verification: VerificationReport,
    pub resources: ResourceSummary,
    pub warnings: Vec<CaptureWarning>,
    pub timings: CaptureTimings,
}
```

`CaptureArtifact` is tagged as file or bytes. Memory artifacts remain subject to
the request's memory ceiling.

```rust
pub struct SourceSummary {
    pub requested_url: RedactedUrl,
    pub requested_url_sha256: ContentDigest,
    pub final_url: RedactedUrl,
    pub final_url_sha256: ContentDigest,
}
```

`RedactedUrl` preserves the scheme, host, port, path, and non-sensitive query
shape needed for diagnostics. It removes credentials and configured secret
query values. The digests cover the complete canonical URLs so callers can
correlate captures without persisting those values.

### 15.3 Browser environment

```rust
pub struct BrowserEnvironment {
    pub viewport: Viewport,
    pub locale: String,
    pub timezone: String,
    pub color_scheme: ColorScheme,
    pub reduced_motion: ReducedMotion,
    pub user_agent: UserAgentPolicy,
}
```

The default desktop profile is explicit and versioned:

- Viewport 1440 by 900 CSS pixels
- Device scale factor 1
- Locale `en-US`
- Timezone `UTC`
- Light color scheme
- Reduced motion enabled during the freeze stage
- User agent from the selected Chromium build

### 15.4 Policies

```rust
pub enum MissingResourcePolicy {
    Warn,
    Fail,
}

pub enum VerificationMode {
    Static,
    Offline,
}

pub enum NetworkPolicy {
    Standard,
    Server,
    Unrestricted,
    Custom(NetworkRules),
}

pub enum LazyLoadPolicy {
    Disabled,
    ViewportSweep(ViewportSweepPolicy),
}

pub enum ConflictPolicy {
    Fail,
    Replace,
    Uniquify,
}
```

The executable-script policy for 1.0 is `Strip`. The type remains an enum
internally so future artifact modes can add explicit behavior through a versioned
public decision.

### 15.5 Capture status

```text
Created
  -> Validating
  -> WaitingForBrowser
  -> Navigating
  -> WaitingForReadiness
  -> Collecting
  -> ResolvingResources
  -> Transforming
  -> Encoding
  -> Verifying
  -> Committing
  -> Succeeded
```

Any non-terminal state can transition to:

```text
Cancelling -> Cancelled
Failed
```

Transitions are centralized and exhaustively tested.

### 15.6 Resource outcome

```rust
pub enum ResourceOutcome {
    Embedded {
        digest: ContentDigest,
        media_type: String,
        bytes: u64,
    },
    External {
        url: RedactedUrl,
        url_sha256: ContentDigest,
        reason: ExternalReason,
    },
    Omitted {
        reason: OmissionReason,
    },
    Failed {
        error: ResourceError,
    },
}
```

### 15.7 Capture event

Events use a tagged union:

- `capture.started`
- `browser.ready`
- `navigation.started`
- `navigation.redirected`
- `readiness.changed`
- `frame.collected`
- `resource.discovered`
- `resource.progress`
- `transform.started`
- `artifact.encoding`
- `verification.started`
- `warning`
- `capture.succeeded`
- `capture.failed`
- `capture.cancelled`

Progress reports counts and bytes. It does not invent a percentage when the
total is unknown.

### 15.8 Browser diagnostics

```rust
pub struct BrowserDoctorReport {
    pub schema_version: u32,
    pub ready: bool,
    pub selected: Option<BrowserInfo>,
    pub candidates: Vec<BrowserCandidate>,
    pub managed_cache: ManagedBrowserState,
    pub collector: CapabilityCheck,
    pub output: OutputCapability,
    pub configuration: Vec<EffectiveConfigValue>,
    pub network: NetworkPolicySummary,
    pub recovery: Vec<RecoveryAction>,
}

pub struct BrowserOperationResult {
    pub schema_version: u32,
    pub action: BrowserAction,
    pub browser: Option<BrowserInfo>,
    pub revision: Option<String>,
    pub cache_dir: PortablePath,
    pub candidates: Vec<BrowserCandidate>,
}
```

Candidate records explain compatible, selected, and shadowed states with
stable reason codes. Effective configuration records contain the
field, redacted value, and provenance tier. Recovery actions contain a stable
code, human description, and structured command arguments.

## 16. Capture pipeline

### 16.1 Stage 1: validate

Input:

- Untrusted `CaptureRequest`

Actions:

1. Parse and normalize the URL.
2. Validate schemes and network policy.
3. Validate option combinations.
4. Validate limits.
5. Resolve output semantics.
6. Check explicit destination conflicts.
7. Validate protected secret files.
8. Create a redacted request summary.
9. Establish total deadline and cancellation token.

Output:

- `ValidatedCaptureRequest`

Failures:

- Invalid URL
- Incompatible option
- Forbidden address
- Invalid output
- Unreadable secret file
- Invalid duration or resource limit

### 16.2 Stage 2: acquire browser

Actions:

1. Resolve explicit browser path or endpoint.
2. Locate a compatible managed or system browser.
3. Provision a managed browser when policy permits.
4. Start or lease a browser process.
5. Create an isolated browser context.
6. Disable downloads.
7. Apply proxy, locale, timezone, and viewport settings.
8. Register lifecycle and network observation.
9. Register collector bindings.
10. Install the document-start hook.

Output:

- `BrowserLease`
- `PageSession`
- `BrowserInfo`

The lease owns context cleanup. The browser service owns shared process
lifecycle.

### 16.3 Stage 3: navigate

Actions:

1. Enable network and lifecycle events before navigation.
2. Apply cookies before the first request.
3. Apply scoped headers through browser interception.
4. Navigate.
5. Validate each redirect against network policy.
6. Track request and response metadata.
7. Attach child frame targets.
8. Detect target crash or detachment.

Output:

- `NavigationObservation`

Automatic navigation retry is disabled. A page navigation can produce external
side effects even when it uses GET.

### 16.4 Stage 4: settle

Default `render-idle` readiness:

1. Wait for the document lifecycle milestone.
2. Track in-flight requests.
3. Ignore WebSocket, EventSource, and classified long-lived connections.
4. Require a network-quiet window.
5. Require a DOM-mutation-quiet window.
6. Wait for `document.fonts.ready`.
7. Run a bounded viewport sweep when lazy loading is enabled.
8. Restore the original scroll position.
9. Require a second quiet window.
10. Freeze CSS animations and transitions.
11. Advance to the next animation frame.
12. Record the settle reason and elapsed time.

`network-idle` waits for DOM content loaded, then requires zero finite requests
for the configured network-quiet window. WebSocket, EventSource, `blob:`, and
`data:` lifetimes are excluded from the request count. `load` and
`dom-content-loaded` stop at their browser lifecycle milestones.

Readiness uses observable signals. An explicit delay is applied after the
signals and remains inside the total deadline. The delay covers worker
computation that updates the document after request activity ends.

### 16.5 Stage 5: establish capture epoch

Actions:

1. Create a capture epoch ID.
2. Freeze animations in attached frames.
3. Record host monotonic time.
4. Start frame snapshots.
5. Record per-frame start and finish offsets.
6. Collect leaf frames before parent encoding.
7. Report drift beyond policy tolerance.

Output:

- `CaptureEpoch`
- Frame observation set

### 16.6 Stage 6: collect live state

The collector obtains:

- Detached HTML clone
- Source and final URLs
- Doctype
- Title
- Base URL
- Encoding
- Viewport and scroll state
- Form state
- Responsive image selections
- Canvas pixels
- Media state
- CSSOM stylesheets
- Adopted stylesheets
- Open and observed closed shadow roots
- Frame references
- Used-font observations
- Collector warnings

Output:

- `FrameObservation`

### 16.7 Stage 7: construct graphs

Actions:

1. Validate collector payloads.
2. Build the frame graph.
3. Parse each frame document.
4. Apply stable collector node identities.
5. Discover HTML resource references.
6. Parse CSS and discover CSS references.
7. Resolve references against their owning base URL.
8. Create resource records.

Output:

- `Document`
- `FrameGraph`
- `ResourceGraph`

### 16.8 Stage 8: acquire resources

Preferred resource sources:

1. Body observed during navigation
2. Browser-context fetch in the owning frame
3. Capability-gated CDP resource load
4. Host HTTP retrieval when policy explicitly enables it

Actions:

1. Apply address and redirect policy.
2. Open a bounded body stream.
3. Enforce encoded and decoded size limits.
4. Stream into the temporary content store.
5. Hash while writing.
6. Validate MIME information.
7. Deduplicate by content digest.
8. Record provenance and outcome.

Output:

- `ResourceGraph`
- Content-addressed store

URL equality does not determine content identity. Authentication, cookies,
referrer, request body, and `Vary` can change response bytes.

### 16.9 Stage 9: transform

Transformation order:

1. Apply current form and disclosure state.
2. Materialize shadow trees.
3. Materialize canvas and media fallbacks.
4. Resolve document URLs.
5. Resolve CSS imports and resource URLs.
6. Embed resources.
7. Embed frames leaf first.
8. Remove executable scripts and event attributes.
9. Remove meta refresh.
10. Remove the base element after URL resolution.
11. Add restoration data for structurally unstable HTML when required.
12. Add Offprint metadata.
13. Add content security policy.
14. Produce the encoder input.

Output:

- `SafeStaticDocument`

### 16.10 Stage 10: encode

Actions:

1. Create a staging transaction beside the final destination.
2. Stream deterministic HTML.
3. Stream large encoded resources.
4. Embed the manifest.
5. Record byte count and digest.
6. Flush the staging file.

Output:

- `StagedArtifact`

### 16.11 Stage 11: verify

Static verification:

- Parse the artifact.
- Validate manifest version and shape.
- Verify embedded resource references.
- Verify frame references.
- Verify CSP.
- Verify restoration script hashes.
- Verify report and manifest counts.
- Reject forbidden render-fetch URLs under safe-static policy.

Offline verification:

1. Create a fresh browser context.
2. Deny every network request.
3. Open the staged artifact.
4. Wait for images, fonts, and frames.
5. Wait for mutation quiet.
6. Record attempted requests and load failures.
7. Check DOM and state invariants.
8. Produce `VerificationReport`.

### 16.12 Stage 12: commit

Actions:

1. Confirm cancellation has not won the terminal race.
2. Sync the staging file where supported.
3. Commit through a platform-appropriate atomic rename or replacement.
4. Sync the destination directory where supported.
5. Mark the transaction committed.
6. Emit the terminal result.
7. Release content store and browser lease.

Output:

- `CaptureReceipt`

## 17. Chromium backend

### 17.1 Protocol client

Offprint uses a narrow generated CDP client over `tokio-tungstenite`.

The workspace pins the official
[Chrome DevTools Protocol](https://github.com/ChromeDevTools/devtools-protocol)
JSON inputs and their upstream revision. `cargo xtask codegen-cdp` generates
the selected domain types.

Required domains:

- Browser
- Page
- Runtime
- Target
- Network
- Fetch
- IO
- DOM
- DOMSnapshot
- Emulation
- Security
- Storage
- Log

Generated output is checked into source for reproducible builds and reviewed in
dedicated changes. CI regenerates and compares it.

### 17.2 Transport

The transport provides:

- Atomic request IDs
- One-shot pending response channels
- Pending-entry removal when a command future is dropped
- Per-command deadlines
- Bounded WebSocket frames and messages
- Event routing by flat session ID
- Bounded broadcast channels
- Close and error propagation
- Sanitized tracing
- Deterministic shutdown

Connection closure fails every pending command immediately.

### 17.3 Frames and targets

Offprint uses flattened target sessions and recursive automatic attachment.
The CDP
[Target domain](https://chromedevtools.github.io/devtools-protocol/tot/Target/)
provides the auto-attach and flat-session behavior required for
out-of-process iframes.

Every frame target receives:

- Required CDP domain enablement
- Document-start hook
- Collector binding
- Runtime world
- Network policy
- Capture epoch command

### 17.4 Browser process

Local launch uses:

- `--remote-debugging-port=0`
- An ephemeral profile directory
- Browser sandboxing
- Disabled downloads
- Explicit viewport and environment
- A process group on Unix
- A job object on Windows
- A parent-death cleanup mechanism where supported

The launcher reads `DevToolsActivePort` instead of reserving a TCP port itself.

### 17.5 Browser pool

Default service behavior:

- One Chromium process per `Offprint` instance
- One isolated browser context per capture
- Bounded concurrent contexts
- Context teardown after every job
- Process restart after crash
- Configurable process recycling after a measured job threshold
- Idle shutdown on explicit close

Browser reuse is an optimization. Capture correctness cannot depend on state
from an earlier context.

### 17.6 DOMSnapshot

The experimental
[DOMSnapshot domain](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/)
can supply layout and flattened DOM information. Offprint uses it as an
auxiliary oracle for diagnostics, layout bounds, and canvas fallback. The
collector's structured frame observation remains canonical because Offprint must
preserve shadow and frame ownership.

## 18. Collector

### 18.1 Role

The collector observes browser-only state. It does not own:

- Capture policy
- Resource failure policy
- Filesystem output
- Artifact encoding
- Retry decisions
- Verification
- User diagnostics

### 18.2 Implementation

- TypeScript source
- Bundled with Bun during development
- One checked generated JavaScript artifact
- Embedded in `offprint-chromium`
- No production Bun dependency
- No third-party runtime dependency in the injected bundle
- Strict TypeScript configuration
- Browser tests against the pinned Chromium build

### 18.3 Document-start hook

The hook wraps `attachShadow` and retains closed roots in a private weak map.
It preserves the original call contract and releases references when documents
are torn down.

The hook version participates in capability negotiation.

### 18.4 Collector purity

The collector creates a detached clone and applies Offprint marker identities
to that clone.

Reads from the live page transfer:

- DOM properties that are not reflected as attributes
- Shadow contents
- Canvas data
- Media state
- Layout state
- CSSOM state

The fixture suite verifies that collection leaves the live page observably
unchanged.

### 18.5 Protocol handshake

The handshake includes:

- Protocol major and minor
- Capture ID
- Host build digest
- Collector build digest
- Requested capabilities
- Available capabilities
- Maximum chunk size

Major mismatch is fatal. Minor compatibility requires every requested
capability and field.

### 18.6 Chunking

The host pulls bounded chunks:

```text
prepare -> describe -> read chunk N -> acknowledge -> release
```

Each chunk includes:

- Capture ID
- Frame ID
- Sequence number
- Total count
- Payload length
- Payload checksum

The initial encoding is JSON for inspection and fixture readability. A binary
encoding requires measured evidence and a protocol version.

## 19. Document and resource processing

### 19.1 HTML model

Offprint uses [html5ever](https://github.com/servo/html5ever) with a custom arena
tree. The arena provides stable node IDs, deterministic traversal, and explicit
ownership.

```rust
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
}
```

The implementation provides the traversal and selector operations required by
Offprint transforms. It is not a general browser DOM.

### 19.2 CSS model

Offprint evaluates
[Lightning CSS](https://github.com/parcel-bundler/lightningcss) as the typed CSS
parser and visitor implementation. Preservation tests must confirm that unknown
syntax and source ordering survive when transformations and minification are
disabled.

A token-preserving `cssparser` path is the fallback for URL rewriting when a
typed parse cannot preserve an input.

Offprint 1.0 retains all captured CSS. Unused CSS and font elimination are
separate optimization passes with visual evidence.

### 19.3 Resource locations

The resource discovery table covers:

- HTML `src`, `srcset`, `href`, `data`, and `poster`
- SVG `href` and `xlink:href`
- SVG paint, clip, mask, and filter references
- Style attributes
- Style elements
- CSS `url`
- CSS `image-set`
- CSS font sources
- CSS imports
- CSS cursor images
- Nested documents

Each reference records frame, node, location kind, original text, base URL,
resolved URL, and rendering role.

### 19.4 Special schemes

- `data:` is decoded locally under encoded and decoded limits.
- `blob:` is read in the owning frame.
- `file:` requires an explicitly allowed root.
- `javascript:` is removed.
- Fragment references remain local.
- Unsupported schemes receive an explicit outcome.

### 19.5 Content store

The content store:

- Streams bytes to temporary files
- Calculates SHA-256 during write
- Uses content digests for deduplication
- Retains resource provenance separately
- Applies total and per-resource budgets
- Deletes uncommitted content on drop
- Supports deterministic fixture injection

### 19.6 Frames

The frame graph contains:

- Parent frame
- Child order
- Source element
- Requested URL
- Final URL
- Origin
- Sandbox flags
- Captured document
- Terminal outcome

Frames are encoded leaf first into `srcdoc` or another browser-verified
encoding selected by the HTML encoder.

### 19.7 Shadow DOM

Shadow trees are encoded through declarative shadow DOM. Adopted stylesheet
content is placed in the owning shadow scope. Closed roots unavailable because
the document-start hook attached too late receive a warning or failure according
to policy.

### 19.8 Canvas and media

Canvas collection order:

1. `toDataURL` in the collector
2. CDP screenshot clipped to the element bounds
3. Explicit failure outcome

Media collection stores a poster or current-frame bitmap plus selected source
provenance.

### 19.9 Forms

Offprint materializes:

- Text values
- Textarea contents
- Checked state
- Selected options
- Details open state

Sensitive controls such as password inputs are redacted by default and receive
a manifest warning.

### 19.10 Script sanitization

Safe-static transformation removes:

- Executable script elements
- Event handler attributes
- `javascript:` URLs
- Meta refresh
- External preload hints that trigger requests
- Captured service worker bootstrap references

Non-executable JSON and metadata scripts remain when their MIME type is
recognized.

### 19.11 Structurally unstable HTML

The serializer reparses its output and compares structural invariants.
Documents whose browser-corrected tree would change after serialization receive
minimal restoration data and an Offprint-owned restoration script.

The script digest is included in content security policy. The repair is recorded
in the manifest.

## 20. Artifact format

### 20.1 Manifest

The HTML contains:

```html
<script
  id="offprint-manifest"
  type="application/vnd.offprint.manifest+json"
>
{...}
</script>
```

The manifest records:

- Artifact format and version
- Generator name and version
- Redacted requested and final source URL
- Digest of each complete canonical source URL
- Capture timestamp
- Browser product and version
- Environment summary
- Policy digest
- Frame count
- Resource summary
- Warning codes
- Structural repair presence

Sensitive request data is excluded.

### 20.2 Content security policy

The encoder derives CSP from the artifact contents. The default policy blocks:

- Network connections
- External navigation refresh
- Captured scripts
- Unlisted embedded script hashes
- Object execution

Required embedded image, style, font, frame, data, and blob sources are permitted
at the narrowest tested scope.

### 20.3 Deterministic serialization

The encoder fixes:

- Metadata field ordering
- Manifest JSON ordering
- Resource iteration order
- Text encoding
- Line endings
- Data URL encoding
- Generated identifier ordering

Tests inject capture time and ID.

### 20.4 Filesystem transaction

The staging file is created in the destination directory. This keeps commit on
one filesystem.

`ConflictPolicy::Replace` uses the platform's atomic replacement primitive when
available. Offprint reports a capability error when the requested guarantee
cannot be provided.

## 21. Errors and diagnostics

### 21.1 Error record

```rust
pub struct OffprintError {
    pub code: ErrorCode,
    pub message: String,
    pub stage: ErrorStage,
    pub retryable: bool,
    pub details: BTreeMap<String, JsonValue>,
    pub diagnostics_path: Option<PortablePath>,
    pub source: Option<Box<OffprintError>>,
}
```

### 21.2 Error stages

- Validation
- Browser
- Navigation
- Readiness
- Collection
- Resource
- Transform
- Encoding
- Verification
- Commit
- Shutdown
- Internal

### 21.3 Stable code families

```text
offprint.input.*
offprint.config.*
offprint.browser.*
offprint.navigation.*
offprint.readiness.*
offprint.collector.*
offprint.frame.*
offprint.resource.*
offprint.transform.*
offprint.artifact.*
offprint.verification.*
offprint.output.*
offprint.runtime.*
offprint.internal.*
```

### 21.4 Diagnostic bundle

When enabled, a sanitized failure bundle may contain:

- Redacted request
- Browser version
- Capture events
- Frame topology
- Resource outcome table
- Console and protocol errors
- Staging artifact
- Verification request log
- Optional screenshots

Secrets, response authorization data, cookie values, and page form secrets are
excluded.

## 22. Security and privacy

### 22.1 Threat model

Page content, URLs, headers, responses, collector payloads, artifact files, and
remote CDP endpoints are untrusted.

Threats include:

- Browser exploitation
- Server-side request forgery
- DNS rebinding
- Cloud metadata access
- Local file access
- Archive and decompression bombs
- Oversized DOM or resource graphs
- Malformed HTML and CSS
- Secret leakage
- Symlink and path traversal
- Output replacement races
- Malicious artifact scripts
- Remote endpoint impersonation
- Binding lifetime misuse

### 22.2 Browser isolation

- Keep Chromium sandboxing enabled.
- Use a fresh browser context per job.
- Disable downloads.
- Disable local file access unless explicitly scoped.
- Keep profiles ephemeral by default.
- Do not reuse cookies across jobs unless a caller supplies a persistent
  context policy.
- Kill local browser process trees during final shutdown.

### 22.3 Network policy

`Standard`:

- Permits public addresses.
- Permits loopback when the initial URL is loopback.
- Blocks link-local and cloud metadata ranges.
- Blocks a public-to-private redirect.
- Revalidates redirect destinations.

`Server`:

- Permits public destinations.
- Blocks loopback, private, link-local, and metadata ranges.
- Applies DNS and redirect checks to every destination.

Use `Custom` with `allowed_hosts` or `allowed_cidrs` when a server deployment
needs an explicit destination allowlist.

`Unrestricted`:

- Requires explicit configuration.
- Still blocks malformed URLs and enforces resource limits.

### 22.4 Filesystem policy

- Explicit `file:` capture requires allowed roots.
- Canonical containment is checked after resolution.
- Symlink-sensitive operations use atomic platform primitives where possible.
- Output staging uses restrictive permissions.
- Browser profiles use owner-only permissions.
- Managed browser extraction rejects traversal and special-file entries.

### 22.5 Secrets

- Secret values never enter CLI arguments in documented workflows.
- Logs use redacted URLs and header names.
- Manifests exclude authentication state.
- Diagnostic bundles use the same redaction library as live diagnostics.
- Binding debug representations redact secrets.
- Secret buffers are cleared where feasible after browser setup.

### 22.6 Telemetry

Offprint performs no product telemetry by default. Managed browser installation
contacts the configured browser distribution source. Capture network traffic is
limited to the requested page and its permitted resources.

## 23. Resource limits and performance

### 23.1 Candidate 1.0 defaults

These defaults are calibrated before release:

| Limit | Candidate default |
| --- | ---: |
| Total capture duration | 120 seconds |
| Navigation redirects | 20 |
| Frames | 256 |
| DOM nodes | 1,000,000 |
| Resources | 10,000 |
| Individual resource bytes | 64 MiB |
| Total resource bytes | 512 MiB |
| Collector chunk bytes | 1 MiB |
| Concurrent resource streams | 8 |
| In-memory artifact bytes | 64 MiB |
| CSS import depth | 64 |
| Frame depth | 64 |

Limits apply to bytes actually received and decoded. Declared content length is
an early hint, not the enforcement source.

### 23.2 Performance principles

- Stream resource and artifact bytes.
- Parse each resource body once.
- Hash during storage write.
- Reuse parsed resources by digest and base URL where semantics permit.
- Avoid a second full DOM walk when an existing pass can collect the same fact.
- Keep rare diagnostic data behind explicit policy.
- Keep browser event queues bounded.
- Make concurrency configurable and bounded.
- Measure Chrome process memory separately from Rust process memory.

### 23.3 Benchmarks

Benchmarks cover:

- HTML parse and serialize
- CSS parse and URL rewrite
- `srcset` parsing
- Data URL encode and decode
- Resource graph creation
- Content store hashing
- Frame embedding
- Manifest serialization
- End-to-end static article
- End-to-end frame-heavy application
- End-to-end image-heavy page
- Repeated captures through one service

Performance claims require a recorded baseline, environment, browser revision,
input corpus, and before-and-after result.

## 24. Validation strategy

### 24.1 Highest test seam

The primary behavior seam is:

```text
CaptureService::start -> CaptureJob::result -> CaptureReceipt + artifact
```

CLI and binding tests reuse the same fixtures and add interface-boundary checks.

### 24.2 Unit tests

- URL and base resolution
- Network address classification
- `srcset`
- Data URLs
- MIME handling
- Filename normalization
- Policy validation
- State transitions
- Resource outcomes
- Redaction
- Manifest serialization
- Output conflict behavior
- Error conversion

### 24.3 Property tests

- Parse and serialize invariants
- URL resolution
- CSS escape handling
- Filename portability
- Resource deduplication
- Manifest round trips
- Chunk ordering
- Event terminal uniqueness
- Cancellation idempotence

### 24.4 Fuzz targets

- Collector protocol parser
- HTML arena sink
- CSS URL rewriting
- `srcset`
- Data URL decoding
- Manifest parser
- MIME parser
- Filename sanitizer
- Resource graph construction

### 24.5 Hermetic browser fixtures

The fixture server provides:

- Two HTTP origins
- Two HTTPS origins
- Redirects
- Cookies
- Header validation
- Authentication
- Referrer-sensitive responses
- CSP
- Service worker
- Slow responses
- Partial responses
- Compression
- Oversized payloads
- Cache variation
- WebSocket and EventSource
- DNS and address policy fixtures

Tests never require the public internet.

### 24.6 Browser behavior matrix

Required fixtures:

- Static article
- Script-rendered document
- DOM mutation settling
- Lazy images
- Responsive images
- Open shadow root
- Closed shadow root
- Adopted stylesheet
- Same-origin iframe
- Cross-origin OOPIF
- `srcdoc`
- Sandboxed iframe
- Canvas
- Tainted canvas
- WebGL canvas
- Form state
- CSS imports
- CSS import cycle
- Font loading
- Blob resource
- Data resource
- SVG resource graph
- Video poster
- Malformed DOM nesting
- Permanent network activity
- Navigation timeout
- Browser crash
- Frame detachment
- Cancellation in every pipeline stage
- Explicit output conflict
- Atomic replacement
- Network-denied reopen

### 24.7 Differential suite

The same fixture URL is captured by SingleFile CLI and Offprint under the same
pinned Chromium environment.

The suite compares:

- Offline network request count
- DOM invariants
- Frame completeness
- Shadow state
- Form state
- Canvas state
- Resource outcomes
- Screenshot similarity
- Artifact bytes
- Capture duration
- Peak resident memory

Artifact bytes are measured, not expected to match.

The existing Datawrapper article corpus is a local exploratory regression set.
Small synthetic fixtures own release gates.

### 24.8 Language binding contract suite

Rust, Node.js, and Python each run:

1. Construct `Offprint`.
2. Capture the same fixture.
3. Consume at least one event.
4. Verify the result schema.
5. Inspect the manifest.
6. Cancel a second job.
7. Close the service.
8. Confirm process and temporary-state cleanup.

### 24.9 Memory and lifecycle suite

- Repeated captures through one service
- Cancellation at each state
- Browser crash during collection
- Consumer drop during event delivery
- Output error during encoding
- Verification failure
- Binding garbage collection with explicit close
- Binding garbage collection without explicit close
- Process exit during active job

Miri runs on FFI-free crates. Sanitizer builds cover native binding boundaries
and platform process code where supported.

## 25. Agent-native repository

Offprint treats agents and humans as first-class contributors to the same
workflow.

### 25.1 Repository instructions

`AGENTS.md` defines:

- Architecture map
- Domain vocabulary
- Supported commands
- Test selection
- Browser test resource limits
- Generated-file ownership
- Security rules
- Comment and documentation rules
- Release boundaries

`REVIEW.md` records recurring review failures:

- Vacuous tests
- Missing cancellation paths
- Unbounded queues
- Secret leakage
- Browser process leaks
- Platform-specific assumptions
- Generated schema drift
- Binding default drift
- User-reachable panics
- Output transaction violations

### 25.2 One command vocabulary

```console
just setup
just fmt
just lint
just check <crate>
just test <crate>
just test-fixture <fixture-id>
just e2e <group>
just fuzz-smoke
just miri
just codegen
just codegen-check
just docs-check
just release-check
```

Commands accept a narrow target where practical. Browser-heavy suites require an
explicit group or fixture ID.

### 25.3 Machine-readable project state

The repository tracks:

- Fixture manifest and expected capabilities
- Schema versions
- CDP revision
- Managed browser revision and digest
- Public error code registry
- Release target matrix
- Generated artifact inventory
- Benchmark environment metadata

Agents can inspect these records without inferring state from prose.

### 25.4 Command guards

Repository scripts reject:

- Stale schemas, binding contracts, fixture manifests, selected CDP types, and
  collector bundles
- Production files above the authored line limit
- Cargo dependency edges outside the explicit ownership matrix
- Missing package metadata and mismatched license files
- Unpinned workflow actions
- Broken relative Markdown links
- `todo!`, `unimplemented!`, and repository-private helper names in shipped
  source

### 25.5 Review process

Risky changes receive:

1. One implementation pass
2. One behavior review against the fixture and contract
3. One safety review against lifecycle, cancellation, bounds, and secrets
4. One fix pass
5. Re-execution of the affected boundary tests

Protocol, parser, resource, binding, and unsafe boundary changes require
independent review contexts.

Reviewers receive the diff and public contract. They do not inherit the
implementer's justification as proof.

### 25.6 Compiler and test queues

Large work is divided by crate or fixture group. A queue item contains:

- Owning crate or fixture
- Current compiler or test failure
- Expected contract
- Allowed files
- Required validation
- Terminal result

Queue workers cannot add stubs, weaken tests, skip fixtures, or change public
contracts to make a failure disappear.

## 26. CI and release

### 26.1 Pull-request lanes

Fast lane:

- Rust format
- Collector format
- Clippy
- Unit tests
- Protocol fixtures
- Schema freshness
- CDP freshness
- Collector bundle freshness
- Dependency license and advisory checks
- Unused dependency check
- Markdown and local link checks
- Whitespace and wording checks

Platform lane:

- Linux x86-64
- Linux arm64
- macOS arm64
- macOS x86-64
- Windows x86-64
- Windows arm64

Browser lane:

- Pinned Chrome for Testing
- Sharded fixture groups
- Serialized jobs within each constrained group
- Failure artifacts retained
- Process-leak audit after each group

### 26.2 Scheduled lanes

- Latest Chromium canary
- Full differential corpus
- Datawrapper exploratory corpus
- Fuzz smoke
- Longer fuzz campaigns
- Miri
- Minimum supported Rust version
- Dependency audit
- Performance baseline comparison
- Repeated-capture memory test

### 26.3 Binary release matrix

Initial native archives:

- `x86_64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Every archive includes:

- `offprint`
- README
- License
- Checksums
- Build metadata

### 26.4 Node.js packages

The npm release includes a root package and per-platform native packages.
The root package selects the correct addon and exposes ESM and CommonJS entry
points. CI imports the published package shape in each supported Node.js major.

### 26.5 Python wheels

Maturin builds wheels for the supported platform matrix. CI installs each wheel
into a clean environment, imports `offprint`, performs a fixture capture, and
closes the service.

### 26.6 Supply chain

Releases include:

- SHA-256 checksums
- Software bill of materials
- Build provenance
- Signed tags
- Managed browser revision and digest
- Dependency license report

### 26.7 Release gate

A version is releasable when:

- Required CI lanes are green.
- Affected tests executed on every supported platform.
- Generated files are current.
- Browser and binding install tests pass.
- No required fixture is skipped.
- No user-reachable panic is known.
- No unresolved high-severity advisory affects the shipped path.
- The release candidate has completed repeated-capture and offline-verification
  runs.

## 27. Implementation program

### Phase 0: freeze contracts and de-risk dependencies

Deliverables:

- Accepted `SPEC.md`
- Feature and parity matrix
- Fixture inventory
- Three trial fixtures
- Dependency evaluation records
- Security threat model
- Source provenance ledger
- Initial performance baseline

Spikes:

- Narrow CDP generator and transport
- Chromium process cleanup on each platform
- OOPIF attachment
- Collector chunk protocol
- html5ever arena
- Lightning CSS preservation
- Tainted canvas screenshot fallback
- Offline denied-network verification
- Node and Python DTO mapping prototype

Trial fixtures:

1. A static article with fonts and responsive images
2. A dynamic application with frames, shadow DOM, form state, and canvas
3. An adversarial page with permanent requests, large resources, and malformed
   markup

Exit gate:

- Each architectural choice has executable evidence.
- Every 1.0 behavior maps to a fixture.
- Binding prototypes can map request, event, result, and error records.
- Browser cleanup works after success, failure, and interruption.
- License and provenance policy is accepted.

### Phase 1: workspace and canonical model

Deliverables:

- Cargo workspace
- Pinned toolchain
- Workspace lints
- `just` commands
- `AGENTS.md`
- `REVIEW.md`
- `offprint-model`
- Schema generator
- Error code registry
- CLI help skeleton
- CI fast lane

Exit gate:

- Supported hosts compile the skeleton.
- JSON Schema and fixtures regenerate deterministically.
- Crate boundary tests pass.
- Production crates reject `todo!` and `unimplemented!`.

### Phase 2: browser lifecycle

Deliverables:

- CDP code generation
- WebSocket transport
- Chromium discovery
- Managed-browser metadata
- Local launch
- Headless launch by default
- Remote attach
- Browser context
- Process tree cleanup
- `BrowserService`
- `doctor`

Exit gate:

- Navigate and close a local fixture on Linux, macOS, and Windows.
- A packaged CLI with the managed browser completes a fresh capture.
- Remote attach completes the same fixture.
- Pending commands fail promptly after disconnect.
- No process remains after interruption.

### Phase 3: navigation and readiness

Deliverables:

- Request and lifecycle observation
- Redirect policy
- In-flight request tracker
- Mutation quiet tracker
- Font readiness
- Lazy viewport sweep
- Animation freeze
- Readiness result

Exit gate:

- Permanent sockets do not block readiness.
- Dynamic fixtures settle through observable signals.
- Timeout and cancellation return typed terminal results.
- Readiness diagnostics explain the transition.

### Phase 4: collector protocol

Deliverables:

- TypeScript collector
- Bun bundle
- Document-start hook
- Protocol handshake
- Chunk pull
- Checksums
- Buffer release
- Detached top-frame clone

Exit gate:

- Rust and TypeScript fixtures pass both directions.
- Large payloads avoid one giant binding message.
- Version mismatch fails before collection.
- Collection leaves the live top frame unchanged.

### Phase 5: frames and browser state

Deliverables:

- Flat sessions
- OOPIF attachment
- Frame graph
- Same-origin frame collection
- Cross-origin frame collection
- Shadow roots
- Adopted stylesheets
- Forms
- Images
- Canvas
- Media
- CSSOM

Exit gate:

- Required state fixtures round-trip.
- Frame failures have explicit outcomes.
- Closed shadow collection reports hook timing.
- Page mutation audit passes.

### Phase 6: document and resource graphs

Deliverables:

- Arena DOM
- HTML parser
- CSS parser
- Resource discovery table
- URL resolution
- Content store
- Browser resource retrieval
- Limits
- Deduplication
- Provenance

Exit gate:

- Every controlled reference has an outcome.
- Resource limits apply before unbounded allocation.
- Property and fuzz tests pass.
- Authentication-sensitive resources retain correct formats.

### Phase 7: transformation

Deliverables:

- Form state
- Shadow materialization
- Canvas and media fallback
- CSS rewriting
- Frame embedding
- Script sanitization
- Base URL handling
- Structural repair
- CSP generation

Exit gate:

- Safe-static fixtures contain no forbidden render-fetch URL.
- Offline DOM invariants pass.
- Controlled screenshot comparisons remain within accepted tolerance.
- Every transformation warning is structured.

### Phase 8: artifact transaction and verification

Deliverables:

- HTML encoder
- Manifest
- Static verifier
- Offline verifier
- Staging transaction
- Atomic commit
- Result report
- Artifact inspector

Exit gate:

- Failure never creates the requested destination.
- Every controlled artifact opens with zero network requests.
- Manifest and result counts agree.
- Cancellation rolls back each stage.

### Phase 9: CLI release candidate

Deliverables:

- `capture`
- `verify`
- `inspect`
- `doctor`
- `browser`
- `completion`
- Configuration
- Secret files
- Human diagnostics
- JSON output
- Exit statuses
- Shell packages

Exit gate:

- CLI boundary tests cover stdout, stderr, JSON, exit status, and precedence.
- Clean-machine captures pass on each supported OS.
- Help text and README examples match the binary.
- Browser installation verifies its digest.

### Phase 10: Rust API stabilization

Deliverables:

- Public API review
- Rustdoc
- Doctests
- SemVer checks
- Custom backend advanced API
- Capture event backpressure tests
- Repeated-service lifecycle tests

Exit gate:

- Public API contains no raw CDP or parser types.
- Examples compile.
- Service shutdown is deterministic.
- A compatibility report approves 1.0 naming and defaults.

### Phase 11: Node.js binding

Deliverables:

- napi-rs addon
- TypeScript declarations
- ESM and CommonJS loaders
- Prebuilt platform packages
- Async jobs and events
- Structured errors
- Node.js examples

Exit gate:

- Contract suite matches Rust results.
- Supported Node.js versions install without a Rust toolchain.
- Event consumers cannot stall capture.
- Explicit close and finalizer tests pass.

### Phase 12: Python binding

Deliverables:

- PyO3 extension
- Maturin package
- Type stubs
- Async context manager
- Async jobs and events
- Structured exceptions
- Python examples

Exit gate:

- Contract suite matches Rust results.
- Supported Python versions install a wheel.
- Event iteration and cancellation pass.
- Explicit close and finalizer tests pass.

### Phase 13: fidelity and scale

Deliverables:

- Optional unused CSS removal
- Optional unused font removal
- Optional hidden element removal
- Alternative media pruning
- Selection capture
- Batch scheduler
- Crawl scheduler
- Resume manifest

Exit gate:

- Each optimizer has visual and structural evidence.
- Batch failures remain isolated by job.
- Resume produces deterministic pending work.

### Phase 14: artifact ecosystem

Deliverables:

- PDF renderer
- Markdown exporter with relative assets
- ZIP encoder
- Self-extracting encoder
- MHTML support

Exit gate:

- Every encoder has an independent verifier.
- Multi-output APIs preserve one capture policy and one resource report.
- Node.js and Python expose the same artifact formats.

## 28. Source provenance and licensing

The inspected SingleFile source is licensed under AGPL-3.0-or-later. Offprint
has been designed after direct source inspection, so the project does not claim
a clean-room process.

Initial Offprint source SHOULD use AGPL-3.0-or-later unless a qualified legal
review approves another licensing structure. Language bindings and distributed
packages follow the accepted project license or an approved dual-license plan.

The repository maintains a provenance ledger for:

- Copied or adapted code
- Protocol definitions
- Generated browser definitions
- Test fixtures derived from external projects
- Vendored assets
- Dependency licenses

Behavioral inspiration alone is recorded in architecture decisions. Source
adaptation includes exact origin, revision, license, and local changes.

## 29. Risk register

| Risk | Consequence | Control | Release evidence |
| --- | --- | --- | --- |
| Dynamic page never settles | Hung jobs | Observable quiet windows and total deadline | Permanent-activity fixture |
| OOPIF detaches during capture | Missing frame | Typed frame outcome and cancellation-safe target map | Detachment fixture |
| Collector mutates page | Capture changes source | Detached clone and live mutation audit | Mutation observer fixture |
| Resource body unavailable | Fidelity gap | Retrieval ladder and explicit outcome | Cache and auth fixtures |
| HTML reparses differently | Corrupt structure | Structural comparison and minimal repair | Malformed HTML corpus |
| CSS parser loses syntax | Visual regression | Preservation mode and token fallback | CSS corpus and screenshots |
| Browser process survives | Resource leak | Owned process tree guard | Repeated interrupt test |
| Binding callback blocks | Capture stall | Bounded channel and host scheduler | Slow consumer test |
| Panic crosses FFI | Host process failure | Typed errors and unwind containment | Binding fault injection |
| Public URL reaches private service | SSRF | Address, redirect, and DNS policy | Network policy fixtures |
| Artifact executes captured script | Security exposure | Script sanitization and CSP | Offline execution fixture |
| Output replaces existing file early | Data loss | Same-directory staging and atomic commit | Failure injection matrix |
| Generated contract drifts | SDK mismatch | Single model and freshness CI | Cross-language fixture suite |
| Managed browser supply chain changes | Compromised runtime | Pinned revision and digest | Install verification |
| Chromium update changes behavior | Fidelity regression | Pinned release and canary lane | Browser comparison report |
| Large page exhausts memory | Worker failure | Streaming and byte, node, frame limits | Oversized fixture |
| License provenance is unclear | Distribution risk | Provenance ledger and release review | License report |

## 30. Architecture decision records

The implementation begins with these accepted decisions:

1. CLI-first native product
2. Rust service API as the canonical execution seam
3. Thin observational TypeScript collector
4. Chromium CDP backend
5. Narrow generated CDP client
6. html5ever arena document
7. Typed CSS parser with preservation fallback
8. Safe-static HTML as the initial artifact
9. Offline verification before commit
10. Transactional output
11. Stable public DTO schema
12. Thin napi-rs and PyO3 bindings
13. Stable Rust toolchain
14. Bun for collector development and bundling
15. Hermetic browser fixtures
16. SingleFile as differential oracle
17. AGPL-compatible initial licensing posture

Each decision record states context, decision, consequences, rejected options,
and the evidence that could trigger reconsideration.

## 31. Offprint 1.0 definition of done

Offprint 1.0 is complete when:

- `offprint capture URL -o FILE` produces a verified HTML artifact.
- The default artifact reopens with zero network requests.
- Required frame, shadow, form, canvas, media, image, font, and CSS fixtures
  pass.
- Every discovered resource has a typed outcome.
- Explicit output uses a verified atomic transaction.
- Cancellation cleans every owned resource.
- Repeated captures have bounded process and memory behavior.
- CLI stdout, stderr, JSON, and exit statuses pass boundary tests.
- The Rust service API is documented and SemVer-reviewed.
- Public records are ready for binding generation.
- Linux, macOS, and Windows release archives pass clean-machine capture tests.
- Managed Chromium installation verifies its digest.
- Required CI lanes execute with no skipped required fixture.
- Fuzz, Miri, dependency, license, and provenance checks pass.
- README commands and examples match the release candidate.

## Appendix A: canonical Rust workflow

```rust
use offprint::{CaptureRequest, Offprint};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::builder()
        .profile("strict")
        .build()?;

    let request = CaptureRequest::builder("https://example.com")?
        .output("example.html")
        .build()?;

    let job = offprint.captures().start(request).await?;
    let result = job.result().await?;

    assert_eq!(result.verification.mode, VerificationMode::Offline);
    offprint.close().await?;
    Ok(())
}
```

## Appendix B: canonical configuration

```toml
default_profile = "default"

[browser]
channel = "managed"
headless = true

[profile.default]
verification = "offline"
missing_resources = "warn"
network_policy = "standard"

[profile.default.environment]
viewport = { width = 1440, height = 900, scale = 1 }
locale = "en-US"
timezone = "UTC"
color_scheme = "light"

[profile.default.readiness]
mode = "render-idle"
network_quiet = "500ms"
mutation_quiet = "300ms"
lazy_load = "viewport-sweep"

[profile.default.limits]
duration = "2m"
redirects = 20
frames = 256
nodes = 1000000
resources = 10000
resource_bytes = "64MiB"
total_resource_bytes = "512MiB"
concurrent_resources = 8
```

## Appendix C: required implementation checks

Every behavioral change answers:

1. Which public contract changes?
2. Which service owns the behavior?
3. Which terminal paths release acquired resources?
4. Which limits apply before allocation?
5. Which fixture fails before the change?
6. Which sibling modes and platforms share the behavior?
7. Which generated schemas or bindings change?
8. Which diagnostics expose the failure safely?
9. Which benchmark detects a cost on the common path?
10. Which documentation example must remain aligned?
