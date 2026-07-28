# Rust public API

The `pageknot` crate exposes one service handle, three service views, one
capture builder, one job handle, and the canonical model records.

```rust
use pageknot::PageKnot;

# async fn example() -> pageknot::Result<()> {
let pageknot = PageKnot::builder().build()?;
let result = pageknot
    .capture("https://example.com")?
    .save("example.html")
    .await?;

assert!(result.verification.passed);
pageknot.close().await?;
# Ok(())
# }
```

## Service surface

| API | Contract |
| --- | --- |
| `PageKnot::builder()` | Configures first-use managed browser provisioning, browser selection, profiles, runtime limits, and deterministic dependencies |
| `PageKnot::capture(url)` | Builds a one-shot request with the selected profile |
| `PageKnot::captures()` | Starts one-page jobs, bounded batches, and breadth-first crawls |
| `PageKnot::artifacts()` | Inspects, verifies, and exports artifact variants |
| `PageKnot::browsers()` | Discovers, installs, removes, diagnoses, and closes browser instances |
| `PageKnot::close()` | Cancels active jobs, waits for cleanup, and releases owned browser resources |

### `CaptureBuilder::wait_until` and `CaptureBuilder::delay`

```rust
pub fn wait_until(self, mode: ReadinessMode) -> Self
pub fn delay(self, duration: Duration) -> Self
```

`wait_until` selects `RenderIdle`, `NetworkIdle`, `Load`, or
`DomContentLoaded`. `delay` adds a host-timed pause after that condition. The
readiness condition and delay share the total deadline configured by
`CaptureBuilder::timeout`. Captures default to `RenderIdle` with no added
delay. `NetworkIdle` excludes WebSocket, EventSource, `blob:`, and `data:`
lifetimes from its finite request count. Use `delay` when worker computation
updates the document after request activity ends.

### `CaptureBuilder::selector`

```rust
pub fn selector(self, selector: impl Into<String>) -> Self
```

Captures the first element matched by `document.querySelector` in the top-level
document. The capture keeps the document head and the matched element's
ancestor chain so stylesheet and layout selectors retain their context.

Calling `selector` sets page scope. A later `scope` call replaces the selector.
The capture future returns `pageknot.selector.invalid` for invalid CSS syntax
and `pageknot.selector.not_found` when the document has no match.

File outputs use `ConflictPolicy::Replace` by default. PageKnot verifies the
staged capture before atomically replacing the destination. Call
`CaptureBuilder::conflict` with `Fail` or `Uniquify` when the caller needs a
different policy.

`PageKnot` clones share one runtime. `close()` is idempotent. Operations started
after close return a structured runtime error.

## Capture jobs

`CaptureService::start` validates the request before returning a `CaptureJob`.
The job exposes:

- `id()` for correlation
- `status()` for the latest lifecycle state
- `events()` for an ordered `Stream<Item = CaptureEvent>`
- `cancel()` for an idempotent cancellation request
- `wait()` for the single terminal `CaptureResult`

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

## Artifact variants

`ArtifactService::export` accepts one verified HTML artifact and an
`ArtifactExportRequest`. It verifies the source once, derives the requested
PDF, Markdown, ZIP, compressed HTML, and MHTML variants, then runs each
format-specific verifier before commit.

`ArtifactService::verify_variant` checks an exported path through the verifier
owned by its `ArtifactVariantKind`.

## Records and errors

The `pageknot` crate reexports the canonical records from `pageknot-model`.
Serialized field names and enum values are part of the versioned schema
contract in [`schemas`](../schemas).

Every public operation returns `pageknot::Result<T>`. `PageKnotError` provides:

- A stable `pageknot.*` code
- The failing pipeline stage
- A redacted message
- Retryability
- Structured details
- An optional causal error
- An optional diagnostic bundle path

Capture results contain the terminal status, redacted source URLs and digests,
artifact record, offline verification evidence, complete resource outcome
counts, structured warnings, and stage timings.

## Custom browser backends

`PageKnotBuilder::browser_backend` accepts an implementation of
`BrowserBackend`. The backend owns browser acquisition, contexts, pages,
resource streams, offline reopen, and deterministic close behavior.

Custom backends implement the browser-independent types reexported by
`pageknot`. Chrome DevTools Protocol records and parser internals remain inside
their owning crates.

## Compatibility policy

The 0.1 release establishes the naming and defaults reviewed for the 1.0
contract:

- `PageKnot` is the root service noun.
- `CaptureRequest -> CaptureService -> CaptureJob -> CaptureResult` is the
  execution seam.
- Builders own optional request configuration.
- Service methods own operations with lifecycle or I/O.
- Model records own the serialized Rust, CLI, Node.js, and Python contract.
- Every public record field, enum value, default, error code, and lifecycle
  guarantee receives SemVer review.
- The minimum supported Rust version is 1.97.

After the first published tag, release CI compares public Rust APIs against the
latest compatible release. Schema freshness and binding contract tests protect
the serialized surface in every release.
