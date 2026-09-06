# Rust public API

The `offprint` crate exposes one service handle, three service views, one
`Capture` operation, one job handle, and the canonical model records.

This page describes the current alpha source checkout. The crate requires Rust
1.97 or newer.

```rust
use offprint::Offprint;

# async fn example() -> offprint::Result<()> {
let offprint = Offprint::new()?;
let result = offprint
    .capture("https://example.com")?
    .save("example.html")
    .await?;

assert_eq!(result.verification.mode, offprint::VerificationMode::Offline);
offprint.close().await?;
# Ok(())
# }
```

## Service surface

| API | Contract |
| --- | --- |
| `Offprint::builder()` | Configures first-use managed browser provisioning, browser selection, profiles, runtime limits, and deterministic dependencies |
| `Offprint::capture(url)` | Builds a one-shot request with the selected profile |
| `Offprint::captures()` | Starts one-page jobs, bounded batches, and breadth-first crawls |
| `Offprint::artifacts()` | Inspects, verifies, and exports artifact formats |
| `Offprint::browsers()` | Discovers, installs, removes, diagnoses, and closes browser instances |
| `Offprint::close()` | Cancels active jobs, waits for cleanup, and releases owned browser resources |

### `Capture::wait_until` and `Capture::delay`

```rust
pub fn wait_until(self, mode: ReadinessMode) -> Self
pub fn delay(self, duration: Duration) -> Self
```

`wait_until` selects `RenderIdle`, `NetworkIdle`, `Load`, or
`DomContentLoaded`. `delay` adds a host-timed pause after that condition. The
readiness condition and delay share the total deadline configured by
`Capture::timeout`. Captures default to `RenderIdle` with no added
delay. `NetworkIdle` excludes WebSocket, EventSource, `blob:`, and `data:`
lifetimes from its finite request count. Use `delay` when worker computation
updates the document after request activity ends.

### `Capture::selector`

```rust
pub fn selector(self, selector: impl Into<String>) -> Self
```

Captures the first element matched by `document.querySelector` in the top-level
document. The capture keeps the document head and the matched element's
ancestor chain so stylesheet and layout selectors retain their context.

Calling `selector` sets page scope. A later `scope` call replaces the selector.
The capture future returns `offprint.selector.invalid` for invalid CSS syntax
and `offprint.selector.not_found` when the document has no match.

File outputs use `ConflictPolicy::Fail` by default. Offprint verifies the
staged capture before publication. Call `Capture::conflict` before `save` when
the caller needs replacement or a unique path.

`Offprint` clones share one runtime. `close()` is idempotent. Operations started
after close return a structured runtime error.

## Capture jobs

`CaptureService::start` validates the request before returning a `CaptureJob`.
The job exposes:

- `id()` for correlation
- `status()` for the latest lifecycle state
- `events()` for an ordered `Stream<Item = CaptureEvent>`
- `cancel()` for an idempotent cancellation request
- `result()` for the successful `CaptureReceipt`

Late event subscribers receive retained lifecycle events and the latest
resource progress value. Progress can be coalesced under backpressure. A
terminal event appears once, then the stream ends.

Cancellation and atomic commit share one terminal decision. Cancellation wins
before commit, or the verified commit completes successfully.

## Batch and crawl scheduling

`CaptureService::batch` runs complete `CaptureRequest` values with bounded
concurrency. Each job receives one terminal result. Resume manifests bind
persisted outcomes to the request digest, so changed requests are scheduled
again.

`CaptureService::crawl` follows links in deterministic breadth-first order.
`CrawlRequest` bounds page count, depth, concurrency, and origin scope. The
scheduler derives one output path per URL and persists the pending frontier
after each terminal page.

## Artifact formats

`ArtifactService::export` accepts one verified HTML artifact and an
`ExportRequest`. It verifies the source once, derives the requested
PDF, Markdown, ZIP, self-extracting HTML, and MHTML formats, then runs each
format-specific verifier before commit.

PDF export preserves Chromium's selectable text, links, tagged structure, and
outline. It adds document properties and XMP metadata derived from the source
HTML and capture manifest.

`ArtifactService::verify_format` checks an exported path through the verifier
owned by its `ArtifactFormat`.

## Records and errors

The `offprint` crate reexports the canonical records from `offprint-model`.
Serialized field names and enum values are part of the versioned schema
contract in [`schemas`](../schemas). This contract round uses public schema 2
and Offprint HTML format 2.

Every public operation returns `offprint::Result<T>`. `OffprintError` provides:

- A stable `offprint.*` code
- The failing pipeline stage
- A redacted message
- Retryability
- Structured details
- An optional causal error
- An optional diagnostic bundle path

`CaptureReceipt` is a success record. It contains redacted source URLs and
digests, the published artifact, evidence for the selected verification mode,
complete resource outcome counts, structured warnings, and stage timings.
Capture failure and cancellation return `OffprintError`.

## Custom browser backends

`OffprintBuilder::browser_backend` accepts an implementation of
`offprint::ports::BrowserBackend`. The backend owns browser acquisition, contexts, pages,
resource streams, offline reopen, and deterministic close behavior.

Custom backends implement the browser-independent contracts under
`offprint::ports`. Chrome DevTools Protocol records and parser internals remain inside
their owning crates.

## Compatibility policy

The current alpha uses the naming and defaults reviewed for the 1.0 contract:

- `Offprint` is the root service noun.
- `CaptureRequest -> CaptureService -> CaptureJob -> CaptureReceipt` is the
  execution seam.
- Builders own optional request configuration.
- Service methods own operations with lifecycle or I/O.
- Model records own the serialized Rust, CLI, Node.js, and Python contract.
- Every public record field, enum value, default, error code, and lifecycle
  guarantee receives SemVer review.
- The minimum supported Rust version is 1.97.

Public APIs, JSON schemas, and artifact formats may change before the first
tagged release. Release CI is configured to compare Rust APIs with the latest
compatible tag once that baseline exists. Schema freshness and binding contract
tests protect the serialized surface on the current branch.

Return to the [documentation index](./README.md) or inspect the generated API
with `cargo doc --open -p offprint`.
