# Service API reference

`Offprint` owns one native runtime and exposes capture, artifact, and browser
services. Rust, Node.js, and Python share serialized records and native
behavior. Host-language names and a few advanced operations differ.

## Construct and close the service

Construct one service for a related workload, reuse it across operations, then
await `close()` during shutdown. Closing rejects new work, cancels active
captures, waits for their terminal cleanup, closes owned contexts, and closes
the local process or remote protocol connection. A second close succeeds.

| Runtime option | Default | Node.js | Python | Behavior |
| --- | --- | --- | --- | --- |
| Browser path | Discovery | `browserPath` | `browser_path` | Select a local Chromium-based executable |
| Remote endpoint | Unset | `cdpUrl` | `cdp_url` | Attach to a trusted [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/) endpoint |
| Cache directory | Platform cache | `cacheDir` | `cache_dir` | Store managed browsers and service cache |
| Browser source | `auto` | `browserSource` | `browser_source` | Restrict automatic discovery to `auto`, `managed`, or `system` |
| Installation | `install-managed` | `browserInstallation` | `browser_installation` | Permit first-use managed installation or use an existing browser |
| Context limit | `4` | `maximumContexts` | `maximum_contexts` | Bound live capture and verification contexts |
| Recycle threshold | `100` | `browserRecycleAfterJobs` | `browser_recycle_after_jobs` | Recycle the shared process after completed jobs |
| Headed mode | `false` | `headed` | `headed` | Show locally launched browser windows |

Browser path and remote endpoint are mutually exclusive. Context limit and
recycle threshold must be greater than zero. A remote endpoint carries the
[remote-browser trust boundary](../guides/remote-browser.md).

Rust configures the same values through `OffprintBuilder`. It also supports
named profile registration, the network policy used to verify existing
artifacts, an injected browser backend, deterministic clocks and capture IDs,
and effective-configuration records for host-owned configuration systems.

## Capture shorthand

The shorthand creates one request from the selected profile and starts it at a
terminal operation.

| Host | Call | Return | Output |
| --- | --- | --- | --- |
| Rust | `offprint.capture(url)?.save(path).await?` | `CaptureReceipt` | File, conflict fails by default |
| Rust | `offprint.capture(url)?.bytes(maximum).await?` | `CaptureReceipt` | Bounded memory |
| Node.js | `offprint.capture(url, options)` | `Promise<CaptureReceipt>` | File named by required `options.output` |
| Python | `await offprint.capture(url, output=path)` | `CaptureReceipt` dictionary | File named by required `output` |

The shorthand exposes profile, timeout, readiness mode, delay, viewport,
strict resource handling, headed mode, conflict policy, network policy,
verification mode, selection, selector, and three removal optimizations. Use a
complete request when credentials, local file roots, custom network rules,
diagnostics, memory output, or exact limits are required.

The canonical memory artifact serializes content as an array of byte values.
Node.js receives `number[]` and can convert it with `Uint8Array.from(content)`.
Python receives `list[int]` and can convert it with `bytes(content)`. Rust keeps
the content as `Vec<u8>`.

Rust's pending `Capture` accepts `output(CaptureOutput)` before `start`.
`into_request()` returns the configured canonical request for further edits or
scheduling. URL parsing happens during construction. `start` validates all
request policies before browser work begins. `save` and `bytes` select an
output and await the terminal result. Dropping either pending operation
requests cancellation.

## `CaptureService`

### `request(url, options?)` in Node.js and Python

Returns a fresh `CaptureRequest` with native defaults and the same options as
`capture`. The optional `output` names a file. Omitting it selects bounded
memory using the profile's artifact byte limit. Python accepts the options as
snake_case keyword arguments.

Request construction is synchronous and starts no browser work. Invalid URLs,
missing profiles, and a closed service raise structured errors. Unknown
options raise `OffprintError` in Node.js and `TypeError` in Python. Edit the
returned record, then pass it to `start` or include it in a batch or crawl
request. The execution method validates the completed request.

Rust uses `offprint.capture(url)?.into_request()` for the same workflow.

### `start(request)`

Validates one complete `CaptureRequest`, prepares capture diagnostics, and
returns a `CaptureJob`. Browser work continues asynchronously. Validation and
diagnostic-directory setup can fail before a job exists.

- Argument: one canonical [`CaptureRequest`](./records.md#capture-input-and-result)
- Return: `CaptureJob`
- Errors: validation, closed runtime, and diagnostic setup errors before return
- Lifecycle: the job owns terminal result access and cancellation

### `batch(request)`

Runs named independent capture requests with bounded concurrency. The result
preserves job order and reports succeeded, failed, and resumed counts. A
per-capture failure becomes a scheduled outcome. Invalid batch structure,
resume state, or service failure rejects the operation.

- Argument: `BatchRequest`
- Return: `BatchResult`
- Defaults: concurrency `4`
- Resume: updates its manifest after each terminal job

### `crawl(request)`

Captures a deterministic breadth-first link graph from one seed request. The
scheduler derives file destinations under `outputDirectory`. It defaults to
100 pages, depth 3, concurrency 4, and same-origin traversal.

- Argument: `CrawlRequest`
- Return: `CrawlResult`
- Browser: local default or injected backend. Remote CDP is rejected
- Verification: preserves the seed request's mode
- Resume: persists completed jobs and pending frontier state

Read [Batch and crawl](../guides/batch-and-crawl.md) for scheduler behavior and
overwrite consequences.

## `CaptureJob`

| Member | Contract |
| --- | --- |
| `id` | Stable capture identifier assigned before browser work |
| `status` | Latest observable `CaptureStatus` |
| `events()` | New independent ordered subscription with guaranteed lifecycle events, up to 16,384 detail events, and latest resource progress |
| `cancel()` | Idempotent cancellation request |
| `result()` | Shared terminal `CaptureReceipt` or `OffprintError` |

Dropping one job handle does not cancel capture. Multiple Rust clones can await
the stored result. Each Node.js or Python event iterator retains the native
event stream and root runtime. A commit that already won terminal arbitration
can complete after a late cancellation request.

Redirect, frame, resource-discovery, and warning detail events can be evicted
after the retention bound. Events report live progress. Persist the terminal
receipt and artifact manifest when the operation needs durable evidence.

## `ArtifactService`

| Operation | Input | Return | Contract |
| --- | --- | --- | --- |
| `inspect` | Offprint HTML | `ArtifactManifest` | Parse and validate the embedded manifest within 64 MiB |
| `verify` | Offprint HTML and mode | `VerificationReport` | Run static checks, then optional network-denied reopen |
| `export` | Offprint HTML and `ExportRequest` | `ExportResult` | Obtain offline source evidence, encode each format, verify each result, commit the set |
| `verifyFormat` or `verify_format` | Export path and format | `FormatVerification` | Run the format-owned verifier without browser reopen |

Node.js and Python artifact operations accept filesystem paths. Rust accepts
`ArtifactSource::File` or bounded bytes for inspect and HTML verification.
Format verification always uses a path. HTML uses `verify`, not the
format-specific verifier.

Rust also exposes:

| Operation | Contract |
| --- | --- |
| `verify_static` | Static HTML verification without browser acquisition |
| `suggested_capture_file_name` | Derive a portable name from an in-memory capture title or source host |
| `commit_capture` | Validate an in-memory receipt and commit it under a conflict policy |
| `export_capture` | Reuse matching offline evidence from a capture receipt before export |

`export_capture` rejects static evidence and mismatched bytes, digest, or byte
count. `commit_capture` checks internal receipt consistency. It does not prove
that a caller-constructed receipt originated from Offprint.

Read [Artifacts and verification](../concepts/artifacts-and-verification.md)
before interpreting proof records. Read [Format reference](./formats.md) for
representation-specific provenance and limits.

## `BrowserService`

| Conceptual operation | Return | Contract |
| --- | --- | --- |
| Ensure | `BrowserInfo` | Resolve and acquire the configured browser, installing the managed revision when policy permits |
| Install | `BrowserOperationResult` | Install the default or named catalog revision after archive verification |
| List | `BrowserOperationResult` | List local selected and compatible system and managed candidates |
| Remove | `BrowserOperationResult` | Remove a managed revision when lease and replacement rules permit |
| Doctor | `BrowserDoctorReport` | Probe selection, collector compatibility, cache, output, configuration, network summary, and recovery actions |
| Close idle | Nothing | Close an idle owned process while keeping the service open |

Rust calls `ensure()`, `install(BrowserInstallRequest)`, `list()`,
`remove(&str, bool)`, `doctor()`, and `close_idle()`. Node.js calls `ensure()`,
`install(revision?)`, `list()`, `remove(revision, { force? })`, `doctor()`, and
`closeIdle()`. Python uses the Node.js argument shape with snake_case method and
keyword names.

`doctor()` returns a report even when `ready` is false. Other operations reject
with `OffprintError` when they cannot satisfy their contract. Removing an active
revision fails. Removing the selected idle revision requires force and another
compatible candidate.

## Errors

Every user-reachable native failure has a code, stage, message, retryability,
optional details object, optional diagnostics path, and optional nested source.
Rust returns `Result<T, OffprintError>`. Node.js rejects with `OffprintError`.
Python raises an `OffprintError` subclass selected by stage.

Branch on code for recovery and treat message text as explanatory. See
[Errors and recovery](./errors.md).

## Rust browser adapter seam

`OffprintBuilder::browser_backend` replaces Chromium selection with a custom
provider-neutral backend. It cannot be combined with a browser path or remote
endpoint. The adapter graph is:

```text
BrowserBackend
    -> BrowserLease
    -> BrowserContext
    -> PageSession
```

`BrowserBackend` owns acquire, doctor, active-browser reporting, and shutdown.
The lease reports browser identity and creates contexts. A context opens one
top-level page. `PageSession` implements credentials, network guard setup,
navigation, readiness, frame observation, visual fallback, bounded resource
loading, offline reopen, optional PDF printing, and close.

`offprint::ports` reexports the browser traits and every request, observation,
network, and resource type needed to implement them. The repository's
[`custom_backend.rs`](../../crates/offprint/tests/custom_backend.rs) is the
compiling end-to-end adapter example. Run it with:

```console
cargo test --locked -p offprint --test custom_backend
```

Adapter implementations must honor cancellation, hard byte and count limits,
network policy, and explicit close ownership. The shared conformance harness is
a tracked development gap, so integration against the service test remains the
current executable contract.

## Exact host signatures

- Rust: `cargo doc --open -p offprint`
- Node.js: [`index.d.ts`](../../bindings/node/index.d.ts)
- Python host API: [`__init__.pyi`](../../bindings/python/python/offprint/__init__.pyi)
- Python record types: [`contracts.py`](../../bindings/python/python/offprint/contracts.py)
- Canonical record fields: [generated JSON Schemas](../../schemas)

Generated declarations own exact signatures. This page owns behavior,
lifecycle, and cross-language differences.
