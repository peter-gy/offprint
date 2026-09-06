# Capture pipeline

The production pipeline has two browser lifetimes. The capture page collects
rendered state and closes before transformation and encoding. Offline
verification later opens a fresh network-denied page against staged bytes.

## Stage trace

```text
validate request and prepare writer
    -> open capture page
    -> navigate and wait
    -> collect frames and resolve resources
    -> close capture page and context
    -> transform document
    -> encode artifact manifest and HTML
    -> read back staged bytes
    -> static verification
    -> optional fresh offline-verification page
    -> claim commit
    -> commit file or return memory
    -> CaptureReceipt
```

## Validate and prepare output

`CaptureService::start` validates the public request before registering a job.
`run_pipeline` then creates the prepared file or memory writer, constructs the
network guard, validates the source URL and local file roots, and establishes
the capture deadline.

File writer construction detects initial conflicts before browser work. Commit
still revalidates staging identity, size, and digest.

## Open and collect the capture page

The runtime admits one browser context, applies browser selection, headed
state, environment, and network policy, then creates a page for capture.

Frame collection owns:

1. Navigation and redirect observation
2. Readiness and optional delay
3. Rendering freeze
4. Frame attachment and observation
5. Resource reference discovery
6. Observed-response and owning-context retrieval
7. Recursive CSS, SVG, and embedded-HTML materialization
8. Resource records, warnings, and aggregate timings

The result is `CapturedPage`, which contains resolved HTML, source summary,
view state, frames, resource summary, resource records, warnings, and timings.

The runtime page closes immediately after collection. Its close path releases
the page, browser context, and context permit before artifact finalization. The
managed revision lease belongs to the pooled process and remains active until
that process closes.

## Transform and encode

Frame collection and resource materialization have already applied form,
shadow, frame, canvas, media, and resource state to HTML.
`offprint-transform` parses that HTML, applies rendering-freeze styles,
sanitizes captured code and request triggers, and detects structural repair.

`offprint-html` then embeds the artifact manifest, exact restoration programs,
and derived content security policy while streaming into the prepared writer.
The writer enforces its artifact byte limit before growth.

## Verify staged bytes

The writer is finalized and read back through the same byte ceiling. Static
verification produces a typed proof that binds bytes, digest, manifest,
structure, owned scripts, and resources. The manifest carries a policy digest,
but the verifier cannot reconstruct the originating request to prove it.

Offline mode stages memory bytes as a temporary HTML file when needed, opens a
new denied-network browser page, waits for stability, and combines browser
observation with the static proof. This verifier page is independent of the
capture page.

## Commit arbitration

After verification, the job transitions to `Committing` and claims the atomic
terminal decision. A cancellation claim before this point prevents commit. A
successful commit claim prevents a late cancellation from changing the result.

The staged writer commits the file transaction or returns a memory artifact.
The pipeline then produces `CaptureReceipt`. Failure attaches sanitized
diagnostics when configured and emits one terminal event.

## Ownership checklist

For every new acquisition, record:

- Owner type
- Acquisition point
- Success release
- Failure release
- Cancellation release
- Drop fallback
- Cleanup deadline

Apply this to browser process, remote connection, managed lease, context,
page, target manager, event task, resource stream, stored content, temporary
artifact, staging writer, and export journal.
