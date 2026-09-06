# Record reference

Canonical records use public schema version 2 and camelCase JSON fields. Rust
types, CLI JSON, Node.js values, and Python dictionaries share these serialized
shapes.

Unknown fields are rejected for many closed request and policy records. Use the
generated [JSON Schemas](../../schemas) for exhaustive field types and enum
values.

## Capture input and result

| Record | Contract | Schema |
| --- | --- | --- |
| `CaptureRequest` | Complete one-page input | [`capture-request.schema.json`](../../schemas/capture-request.schema.json) |
| `ContentPolicy` | Resource, password, scope, selector, optimization, and file-root policy | [`content-policy.schema.json`](../../schemas/content-policy.schema.json) |
| `CaptureEvent` | Tagged progress or terminal event | [`capture-event.schema.json`](../../schemas/capture-event.schema.json) |
| `CaptureReceipt` | Successful terminal result | [`capture-receipt.schema.json`](../../schemas/capture-receipt.schema.json) |
| `VerificationReport` | Static or offline HTML evidence | [`verification-report.schema.json`](../../schemas/verification-report.schema.json) |
| `OffprintError` | Structured failure | [`error.schema.json`](../../schemas/error.schema.json) |

`CaptureRequest` contains `schemaVersion`, `url`, `output`, `browser`, optional
`headed`, `environment`, `readiness`, `content`, optional `credentials`,
`network`, `limits`, `verification`, and `diagnostics`.

`CaptureReceipt` contains `captureId`, redacted `source`, `artifact`,
`verification`, aggregate `resources`, `warnings`, and `timings`. The artifact
is tagged `file` or `bytes`.

## Events and status

`CaptureStatus` uses camelCase lifecycle values from `created` through one of
`succeeded`, `cancelled`, or `failed`.

`CaptureEvent` uses a `type` tag:

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

Progress values are snapshots. Event delivery can coalesce resource progress
under backpressure.

## Artifact records

| Record | Contract | Schema |
| --- | --- | --- |
| `ArtifactManifest` | Embedded HTML provenance and resource inventory | [`artifact-manifest.schema.json`](../../schemas/artifact-manifest.schema.json) |
| `ArtifactVerification` | Uniform CLI verification result | [`artifact-verification.schema.json`](../../schemas/artifact-verification.schema.json) |
| `FormatVerification` | Export-format verifier evidence | [`format-verification.schema.json`](../../schemas/format-verification.schema.json) |
| `ExportRequest` | Requested export set | [`export-request.schema.json`](../../schemas/export-request.schema.json) |
| `ExportResult` | Committed exported representations | [`export-result.schema.json`](../../schemas/export-result.schema.json) |

The artifact manifest contains individual `resourceRecords`. The capture receipt
contains only their aggregate `ResourceSummary` and capture warnings.

### Capture warnings

`CaptureWarning` contains `code`, `message`, and optional `frameId` and
`resourceId` associations. Warnings describe a permitted fidelity gap or
preservation fallback and do not make capture fail under the active policy.

Resource retrieval failures can reuse an `offprint.resource.*` error code as a
warning code. Transform-specific warnings include values such as
`offprint.resource.css_preservation`. Inspect the associated resource record,
then select strict resource handling when the same condition must fail capture.
Warning codes are not an exhaustive subset of the error registry.

## Browser records

| Record | Contract | Schema |
| --- | --- | --- |
| `BrowserDoctorReport` | Readiness, candidates, cache, collector, output, configuration, network, recovery | [`browser-doctor-report.schema.json`](../../schemas/browser-doctor-report.schema.json) |
| `BrowserOperationResult` | Install, list, or remove result | [`browser-operation-result.schema.json`](../../schemas/browser-operation-result.schema.json) |

`BrowserInfo.source` records `managed`, `system`, `explicit`, or `remote`.
Candidate state records `selected`, `compatible`, or `shadowed` with a reason
code, priority, and active lease count.

## Batch, crawl, and resume

| Record | Contract | Schema |
| --- | --- | --- |
| `BatchRequest` | Named complete requests plus concurrency and resume options | [`batch-request.schema.json`](../../schemas/batch-request.schema.json) |
| `BatchResult` | Ordered scheduled outcomes and counts | [`batch-result.schema.json`](../../schemas/batch-result.schema.json) |
| `CrawlRequest` | Seed request, bounds, origin rule, output, resume options | [`crawl-request.schema.json`](../../schemas/crawl-request.schema.json) |
| `CrawlResult` | Breadth-first page outcomes and counts | [`crawl-result.schema.json`](../../schemas/crawl-result.schema.json) |
| `ResumeManifest` | Atomic persisted scheduler checkpoint | [`resume-manifest.schema.json`](../../schemas/resume-manifest.schema.json) |

`BatchJob` is a serialized descriptor. It is distinct from the live
`CaptureJob` handle. `ScheduledCaptureOutcome` is tagged `succeeded`, `failed`,
or `resumed`. The `resumed` counter overlaps terminal counts. A resumed success
increments both `resumed` and `succeeded`. A resumed failure increments both
`resumed` and `failed`.

## Redaction and digests

Source summaries preserve redacted URLs and SHA-256 digests of the complete
canonical URLs. Credential values serialize as `[REDACTED]`. Store original
requests separately when replay requires credentials.

Full URL digests can confirm a guessed secret URL. `BrowserInfo` can contain a
local executable path or redacted remote endpoint. Review manifests, receipts,
doctor reports, and diagnostics for local or internal topology before sharing
them.

`ContentDigest` serializes as lowercase SHA-256 hexadecimal text. Capture IDs
use the `cap_` prefix followed by a ULID, a time-sortable unique identifier.
