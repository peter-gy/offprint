# What is Offprint?

Offprint is a rendered-page capture engine. It opens a URL in
[Chromium](https://www.chromium.org/Home/), observes the page after browser code
has run, embeds the resources used for rendering, and writes a verified HTML
artifact.

```text
input       https://example.com
operation   one capture
artifact    example.html
evidence    CaptureReceipt
```

Run one capture with the command-line interface (CLI):

```console
./target/release/offprint capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the artifact path in human mode and emits the receipt with
`--json`. Service APIs return the receipt directly.

The operation is a **capture**. Its canonical result is an **Offprint HTML
artifact**, a self-contained representation designed to reopen from disk. A
successful capture also returns a **capture receipt** with the artifact digest,
verification evidence, resource counts, warnings, and stage timings.

## What a capture preserves

Offprint reads the rendered browser state rather than the original response
body. A capture can preserve:

- Nodes created or changed by page scripts
- Stylesheets and rules from the CSS Object Model (CSSOM), the browser's live
  stylesheet interface
- Responsive image choices, fonts, SVG images, and other render dependencies
- Same-origin, cross-origin, inline, and sandboxed frames
- Open shadow roots and closed roots observed by Offprint's document-start hook
- Current form controls, selected options, open disclosure elements, and scroll
  position
- Canvas pixels and media poster or current-frame fallbacks
- Source, browser, environment, resource, warning, and policy provenance

Password input values are redacted by default.

Offprint removes captured page scripts, event-handler attributes, automatic
refresh, and request-triggering references from the canonical artifact. Form
and disclosure values become ordinary HTML state, shadow roots become
declarative templates, and exact Offprint-owned programs restore scroll or
structural state. The artifact's
[Content Security Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/CSP)
permits those known programs and blocks unlisted scripts and network
connections.

The project calls this contract **safe-static HTML**. Safe-static describes the
artifact's execution and network boundary. It does not claim that the page is
truthful, harmless to view, or complete when capture warnings report failed
resources.

## The product model

```text
capture request
    -> browser observation
    -> resource resolution
    -> safe-static transformation
    -> Offprint HTML artifact
    -> verification
    -> file commit or memory return
    -> capture receipt
```

A **capture request** holds the source URL, browser environment, readiness
rules, content policy, credentials, network policy, limits, verification mode,
and output selection. A **capture job** is the cancellable handle for the
running capture. An **observation** is the provider-neutral browser state read
before transformation.

Every render-affecting location becomes a **resource reference**. The artifact
manifest stores a **resource record** for each reference, including its outcome
and retrieval provenance. [Resources and fidelity](../concepts/resources-and-fidelity.md)
develops that model.

## Canonical artifact and exports

Offprint HTML is the capture artifact and the source for later operations. You
can inspect its embedded manifest, repeat static or offline verification, or
derive these export formats:

- PDF
- Markdown with content-addressed image assets
- ZIP containing Offprint HTML and a manifest sidecar
- Self-extracting HTML with an executable decompression loader
- MIME HTML (MHTML), a two-part `multipart/related` message for browser import

Each export has a format-specific verifier. Export formats preserve different
subsets of the HTML artifact, so choose one through the
[format reference](../reference/formats.md).

## Interfaces

The native service owns capture behavior. Four interfaces expose it:

| Interface | Best fit |
| --- | --- |
| CLI | Shell use, automation, batch files, and release archives |
| Rust | Direct service composition, memory output, and custom browser ports |
| Node.js | Promise-based applications with typed canonical records |
| Python | `asyncio` applications with stage-specific exception classes |

Continue with the [quickstart](./quickstart.md), or read
[why Offprint uses this pipeline](./why-offprint.md).
