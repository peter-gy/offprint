# Browser and network architecture

`offprint-browser` owns provider-neutral ports and network classification.
`offprint-chromium` implements those ports through Chromium and the Chrome
DevTools Protocol (CDP).

## Browser port

The main ownership chain is:

```text
BrowserBackend
    -> BrowserLease
    -> BrowserContext
    -> PageSession
```

The port exchanges `FrameObservation`, readiness evidence, bounded resource
streams, and offline-browser observations. Implementations must release each
owner on success, failure, cancellation, disconnect, and drop fallback.

The repository has Chromium adapter tests and custom-backend integration tests.
The common-harness gap and its completion condition live in
[Testing](./testing.md#current-conformance-gap).

## Local discovery and process ownership

Discovery applies the configured explicit path, managed candidates, and system
candidates under `BrowserSourcePolicy`. It recognizes Chrome, Chromium, and Edge
with Chromium major 120 or newer.

One runtime owns one local process at a time. Captures use isolated contexts.
Process reuse requires compatible browser selection and headed state. The
runtime bounds contexts, restarts after crash, recycles after the configured job
threshold, and keeps an idle process pooled until `close_idle`, recycle, or
service shutdown.

Local launch uses an ephemeral user-data directory, CDP loopback binding,
disabled downloads, browser sandboxing, process-tree containment, and blocked
direct WebRTC UDP.

## Managed browser

`ManagedBrowserManager` owns catalog lookup, cache locking, archive download,
digest verification, safe extraction, version probe, atomic directory commit,
and cross-process lease markers.

Catalog updates must align:

- `versions.toml` catalog, revision, version, and platform digests
- Managed catalog constants and URLs
- Release platform support
- Installer tests and archive limits
- Documentation compatibility table

## Remote CDP

HTTP discovery tries the version endpoint, target list, then direct WebSocket
resolution. URL user information and common secret query keys are redacted from
diagnostics. Unfamiliar query keys can remain visible.

The caller owns the process and endpoint. Offprint owns its connection,
contexts, pages, target sessions, and cleanup. Remote requests require static
verification and unrestricted address policy. Crawl and coordinator-local file
capture reject remote selection.

## Network enforcement

Network containment has four parts:

1. `NetworkGuard` classifies the seed, redirects, Domain Name System address
   answers, and resource URLs.
2. The validating proxy resolves addresses, requires every answer to pass, and
   connects to an approved address to reduce rebinding risk.
3. Page interception applies scoped headers and blocks unsupported direct
   transports.
4. Offline verification denies every request in a separate browser context.

Cookie scope and local-file-root containment are validated by request and
pipeline code around the guard. Direct transport containment belongs to browser
launch and target configuration.

`NetworkPolicy::Unrestricted` disables address-class restrictions. It does not
disable WebSocket, EventSource, WebRTC, malformed URL, byte, count, or deadline
containment.

## Readiness

`render-idle` combines document completion, bounded viewport sweep, bounded
font and mutation observations, stable finite network activity, optional delay,
and rendering freeze. Font or mutation probes can report false in readiness
evidence after their internal deadline. `network-idle` requires zero finite
requests for the quiet window. Load modes stop at their browser lifecycle
milestones.

All readiness work shares the capture deadline. Navigation has no automatic
retry.

## Resource identity

Resource identity includes request variant and owning frame, not URL alone.
Observed response reuse requires unambiguous identity. Retrieval sources are
inline data, observed response, browser-context fetch, owning-frame read, and
allowed local-file read. There is no production host HTTP fallback for capture
resources.
