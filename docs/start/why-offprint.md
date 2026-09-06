# Why Offprint?

Downloading a response body does not preserve the page a browser displayed.
The visible result may depend on later scripts, live stylesheet rules, lazy
requests, nested frames, form state, canvas pixels, authentication, browser
layout, and time.

## A rendered page is more than its first response

Consider a page that loads an empty application shell, fetches data, renders a
chart into a canvas, and opens a detail panel after user interaction. Saving the
initial HTML loses the data, chart pixels, and open panel. Saving the live
[Document Object Model](https://dom.spec.whatwg.org/), the browser's in-memory
document tree, still leaves stylesheets, fonts, images, and frames on the
network.

Offprint handles that page in distinct stages:

| Problem | Offprint action | Evidence |
| --- | --- | --- |
| Content appears after load | Wait for observable readiness, then read browser state | Readiness events and timings |
| Rendering depends on external bytes | Resolve each resource reference and embed its bytes | Resource records and outcomes |
| Page code can keep running | Remove captured page code and permit exact restoration programs | Static verification and content policy digest |
| A file can still request the network | Reopen it in a network-denied browser context | Offline verification report |
| Failure can corrupt an existing file | Stage and verify before committing the destination | File conflict and transaction result |

The stages expose different guarantees. Resource acquisition can finish with
warnings while the artifact remains self-contained because failed references
are replaced with inert fallbacks. Use the strict missing-resource policy when
the capture must fail on any such fidelity gap.

## Verification is part of capture success

Encoding bytes proves that serialization finished. It does not prove that the
artifact follows Offprint's structure or reopens without requests.

Static verification checks the manifest, content policy, exact owned scripts,
embedded resources, frame accounting, digests, and forbidden request
references. Offline verification performs those checks and then reopens the
artifact with network access denied. Offline is the default mode.

The capture commits a file after the selected verification mode succeeds. A
failed or cancelled capture leaves the requested destination unchanged.

## The request and receipt make the result inspectable

A capture request makes environment, readiness, network, fidelity, limit, and
output choices explicit. A capture receipt records what happened without
requiring callers to parse logs.

The embedded artifact manifest keeps the longer-lived provenance beside the
saved page. It records the redacted source, source digests, browser, browser
environment, resource records, warning codes, frame count, policy digest, and
requested verification mode. The capture receipt carries the full warning
records.

## Where Offprint fits

Offprint fits workflows that need a bounded rendered-page artifact, including
research capture, regression evidence, CI snapshots, authenticated internal
pages, and input to a larger archive.

The product captures pages, bounded batches, and breadth-first crawls as
independent page captures. It is not a network-traffic archive, a general
browser automation API, or a guarantee of pixel-perfect replay for protected
media.

Run the [quickstart](./quickstart.md) to create and verify one artifact. Read
[the capture model](../concepts/capture-model.md) when integrating a long-lived
service or consuming events.
