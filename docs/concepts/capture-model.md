# The capture model

A capture executes one `CaptureRequest` and ends with one terminal result. The
model separates configuration, active work, browser observations, artifact
delivery, and success evidence so callers can reason about each boundary.

```text
CaptureRequest
    -> CaptureJob
    -> Observation
    -> Offprint HTML artifact
    -> VerificationReport
    -> file commit or memory return
    -> CaptureReceipt
```

## Capture request

`CaptureRequest` is the complete, versioned input record. It contains:

- Source URL and browser selection
- Browser environment and optional headed override
- Readiness policy
- Content and credential policy
- Network policy
- Time, count, and byte limits
- Verification mode
- Diagnostic policy
- File or bounded-memory output

`CaptureRequest` rejects unknown top-level fields and an incompatible
`schemaVersion`. Many nested request and policy records are also closed. Check
each generated schema's `additionalProperties` value for its exact field
extension contract. Validation runs before browser work when the required
information is available.

The Node.js and Python bindings expose a shorter capture method that writes one
file. The CLI requires an output and can stream verified bytes to standard
output. Rust exposes file and memory terminals on its fluent `Capture` value.
Use a complete `CaptureRequest` with the capture service for credentials,
custom network rules, complete limits, local file roots, diagnostic
output, or an explicit per-request browser.

## Capture and capture job

The Rust `Capture` type is a pending fluent request. Calling `save`, `bytes`, or
`start` begins browser work.

A **capture job** is the handle returned by `start` for an active or completed
capture. It exposes:

- A stable capture ID
- The current `CaptureStatus`
- Independent event subscriptions
- Idempotent cancellation
- The shared terminal result

Dropping a job handle does not cancel its capture. Call `cancel()` or close the
parent `Offprint` service. Cloned job handles can await the same stored result.

## Lifecycle and events

The job progresses through these status families:

```text
created -> validation -> browser -> navigation -> readiness
        -> collection -> resources -> transform -> encoding
        -> verification -> commit -> succeeded
```

Any active stage can move through cancellation to `cancelled`, or terminate as
`failed`. `CaptureStatus` names lifecycle position. `ErrorStage` names the
owner of a failure and does not map one-to-one to statuses.

Events add browser identity, redirects, readiness milestones, collected frames,
resource progress, warnings, encoding bytes, and terminal evidence. Late
subscribers receive every retained lifecycle and terminal event, up to 16,384
recent detail events, and the latest coalesced resource progress. Redirect,
frame, resource-discovery, and warning details can be evicted beyond that
bound. The stream ends after one terminal event. Treat the receipt and artifact
manifest as durable evidence, not the event stream.

## Observation

An **observation** is provider-neutral browser state collected before document
transformation. It includes rendered HTML, document metadata, frames, scroll
and viewport state, active document selection, visual fallback locations, and
resource observations.

Observation records belong to the browser port. Chrome DevTools Protocol and
collector transport records remain implementation details.

## Commit and cancellation

File delivery stages output beside the destination. Verification runs against
the staged artifact. Cancellation and commit then compete for one terminal
decision:

- Cancellation wins before commit and the destination remains unchanged.
- A commit claim prevents late cancellation from interrupting filesystem
  mutation. Commit can still return an output error.

Memory delivery returns verified bytes and has no filesystem commit. The public
`CaptureArtifact` record identifies whether the receipt contains a file or
bounded bytes.

## Capture receipt

`CaptureReceipt` is the success record. It contains:

- Capture ID
- Redacted requested and final URLs plus digests of the complete URLs
- Artifact delivery, byte count, and digest
- HTML verification report
- Aggregate resource summary
- Structured warnings
- Per-stage timings

Individual resource records live in the artifact manifest, not the receipt.
Read [artifacts and verification](./artifacts-and-verification.md) for the
relationship between the receipt, manifest, and verification records.

## Service lifetime

One `Offprint` service can reuse one local browser process across captures. It
creates an isolated context for capture and, in offline mode, a separate
verification context. The service defaults to four concurrent contexts. The
process recycles after 100 completed jobs by default.

`close()` rejects new work, cancels active jobs, waits for owned contexts, and
closes the browser backend. Await it during application shutdown. Drop-based
cleanup is a fallback and cannot provide the same orderly shutdown guarantee.
