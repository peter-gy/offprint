# Product contract

Offprint captures one rendered URL through Chromium, converts the observation
into canonical Offprint HTML, verifies the selected artifact contract, and
delivers a file or bounded memory value.

Status: alpha. This document describes the current source tree. Planned or
historical behavior has no normative force.

## Public execution seam

```text
CaptureRequest -> CaptureService -> CaptureJob -> CaptureReceipt
```

The CLI, Rust API, Node.js binding, and Python binding call this seam. Browser,
document, resource, format, and output behavior remains behind the service
boundary.

## Canonical nouns

| Term                   | Meaning                                                                                                               |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Capture                | One request execution ending in one terminal result                                                                   |
| Capture request        | Source, browser, environment, readiness, content, credentials, network, limits, verification, diagnostics, and output |
| Capture job            | Cancellable handle for an active or completed capture                                                                 |
| Capture receipt        | Successful terminal summary                                                                                           |
| Observation            | Provider-neutral browser facts collected before transformation                                                        |
| Document               | Internal arena-backed page model                                                                                      |
| Resource reference     | One render-affecting document location                                                                                |
| Resource record        | Artifact-manifest entry for one reference                                                                             |
| Resource outcome       | Terminal state recorded for a resource reference                                                                      |
| Artifact               | Encoded user-visible representation                                                                                   |
| Offprint HTML artifact | Canonical safe-static capture representation                                                                          |
| Export artifact        | Derived PDF, Markdown, ZIP, self-extracting HTML, or MHTML representation                                             |
| Artifact delivery      | File or bounded bytes returned in `CaptureArtifact`                                                                   |
| Artifact manifest      | Embedded `ArtifactManifest` provenance record                                                                         |
| Export format          | PDF, Markdown, ZIP, self-extracting HTML, or MHTML                                                                    |
| Verification mode      | Requested `static` or `offline` HTML check                                                                            |
| Verification report    | Digest-bound HTML evidence                                                                                            |
| Format verification    | Export-format verifier evidence                                                                                       |
| Commit                 | Install verified filesystem output at its destination                                                                 |
| Publish                | Upload packages or release assets to a registry                                                                       |
| Capture profile        | Named bundle of capture defaults                                                                                      |
| Browser environment    | Viewport, locale, timezone, color scheme, reduced motion, and user agent                                              |
| Network policy         | Address and redirect classification rules                                                                             |
| Resume manifest        | Persisted batch or crawl checkpoint                                                                                   |

Qualify overloaded implementation names in prose. `CaptureJob` and `BatchJob`
are different objects. `BrowserSpec`, `BrowserSourcePolicy`, and `BrowserSource`
describe request, discovery policy, and result. `CaptureStatus` and `ErrorStage`
describe lifecycle and failure ownership.

## Current capabilities

- Rendered HTML, CSSOM styles, frames, shadow roots, forms, canvas, media,
  responsive images, fonts, and render dependencies
- Page, active-selection, and CSS-selector scope
- Static and offline HTML verification
- Transactional file output and bounded memory output
- Artifact inspection and verified export to five representations
- Capture jobs, events, cancellation, and shared service shutdown
- Bounded batch scheduling and deterministic breadth-first crawl with resume
- Managed, system, explicit, and remote Chromium-based browsers
- Protected header and cookie inputs
- Versioned JSON, TypeScript, Python, and Rust records

## Product boundaries

Offprint produces rendered-page artifacts. It does not produce a Web ARChive
(WARC), replay arbitrary captured page code, provide a general browser
automation API, or guarantee pixel-perfect protected-media replay.

The current canonical HTML pipeline produces `embedded` and `failed` resource
outcomes. The public model reserves `external` and `omitted`, but safe-static
verification rejects external rendering resources and no production capture
path emits either reserved variant.

## Invariants

### Capture

- Request validation precedes browser registration where inputs permit it.
- Every admitted resource reference receives one terminal record.
- One job emits one terminal event and stores one terminal result.
- Cancellation is idempotent.
- Slow event consumers cannot block capture execution.
- Browser pages, contexts, leases, streams, temporary content, and staging
  outputs have bounded cleanup paths.

### Artifact

- Canonical HTML removes captured page code and permits exact Offprint-owned
  restoration programs.
- Static verification binds format, manifest, content policy, structure,
  resources, and digests.
- Offline verification adds a separate network-denied browser-context reopen.
- File output becomes visible after selected verification succeeds.
- Failed or cancelled capture preserves the prior destination.
- Fixed input and injected clock and ID dependencies produce deterministic
  encoding.

### API

- Frontends call `offprint` services.
- Public serialized records use owned binding-friendly values.
- Closed request and policy records reject unknown fields.
- User-reachable failures return typed `OffprintError` values.
- Panics remain inside binding boundaries and are release blockers.
- Public schema, artifact format, collector protocol, product, browser catalog,
  and CDP revisions evolve independently.

## Interface semantics

The interfaces share native behavior and canonical serialized records. They do
not share identical host-language types:

- Rust owns the complete typed model and advanced ports.
- Node.js exports TypeScript record aliases and one error class.
- Python method arguments use snake_case, while canonical dictionaries remain
  camelCase. Python exposes stage-specific exception subclasses.
- CLI verification JSON uses the uniform `ArtifactVerification` projection.

User documentation owns supported workflows. Generated schemas and host
declarations own exhaustive field and signature inventories.
