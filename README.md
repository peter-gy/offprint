# PageKnot

**Capture a rendered web page as a verified, self-contained file.**

PageKnot is a native headless browser service with a CLI and Rust API. It opens
a URL in Chromium, waits for the page to settle, captures the rendered document
and its resources, and writes one portable HTML file. It then opens that file
with network access blocked to prove that the capture can stand on its own.

```console
pageknot capture https://example.com -o example.html
```

The resulting file opens in a browser, keeps the page's visible state, and
includes a manifest that explains what PageKnot captured.

## Install

Install the native CLI:

```console
cargo install pageknot-cli
```

The first capture uses a cached managed browser or a compatible Chrome or
Chromium installation. When discovery finds neither, PageKnot downloads its
pinned Chrome for Testing build, verifies the archive, and stores it in the
browser cache. Run `pageknot browser install` to fill that cache before the
first capture.

Automatic provisioning is available on Linux x86-64 with glibc, macOS x86-64
and arm64, and Windows x86-64. Release packages are published for the same
targets. On another target, select a compatible executable with
`--browser-path`. Run `pageknot doctor` to inspect the selected browser,
configuration, cache, and output capabilities.

## Capture a page

```console
pageknot capture https://developer.mozilla.org/
```

PageKnot derives a portable filename from the page title and prints the final
path to stdout. Progress and diagnostics go to stderr, so the command composes
cleanly in scripts:

```console
artifact="$(pageknot capture https://example.com --quiet)"
pageknot verify "$artifact"
```

Use an explicit path when another process owns the destination:

```console
pageknot capture https://example.com \
  --output artifacts/example.html \
  --verify offline
```

File outputs replace an existing destination after verification succeeds. A
failed or cancelled capture leaves the existing file unchanged.

Capture the verified HTML representation and commit its PDF rendering:

```console
pageknot capture https://example.com \
  --format pdf \
  --output artifacts/example.pdf
```

PDF capture reuses the offline verification record from the HTML capture,
resolves printable links against the source URL, and preserves Chromium's
selectable text, tagged structure, and document outline. The PDF carries the
HTML title, language, author, description, keywords, source URL, capture time,
and PageKnot provenance in standard document properties and XMP metadata.
PageKnot verifies that metadata and the passive PDF structure before replacing
the destination. Use `--landscape` or `--prefer-css-page-size` to control
printing.

Wait for finite page requests to finish, then allow one additional second for
client-side rendering:

```console
pageknot capture https://example.com \
  --wait-until network-idle \
  --delay 1s
```

`--wait-until` accepts four readiness conditions:

| Condition | Capture begins after |
| --- | --- |
| `render-idle` | Load, lazy loading, fonts, DOM quiet, and network quiet. This is the default. |
| `network-idle` | DOM content loaded, followed by zero finite requests for the configured quiet window. |
| `load` | The browser load milestone. |
| `dom-content-loaded` | The browser DOM content loaded milestone. |

`--delay` applies after the selected condition. Both readiness and delay remain
inside `--timeout`. `network-idle` excludes WebSocket, EventSource, `blob:`,
and `data:` lifetimes from its finite request count. Add `--delay` when worker
computation updates the document after request activity ends. Use `render-idle`
for pages whose fetch or XHR requests stay open continuously.

Capture the first element that matches a CSS selector:

```console
pageknot capture https://www.datawrapper.de/blog/dual-axis-charts-guide \
  --selector "main article" \
  --output article.html
```

Selector capture keeps the document head and the matched element's ancestor
chain, then removes sibling content and unrelated frames. Invalid selectors
fail with `pageknot.selector.invalid`. A selector with no match fails with
`pageknot.selector.not_found`.

PageKnot loads the requested URL in a sandboxed browser and executes the page
while collecting its rendered state. Capture sites you trust, and use a
restricted network policy when URLs come from another user.

## Attach an existing browser

Use a caller-owned Chrome DevTools Protocol endpoint for static capture:

```console
pageknot capture https://example.com \
  --cdp-url http://127.0.0.1:9222 \
  --network-policy unrestricted \
  --verify static
```

Remote capture requires the `unrestricted` network policy and `static`
verification. The caller owns endpoint trust, browser lifecycle, and network
controls for that process. `pageknot doctor --cdp-url <URL>` reports collector
compatibility and the remote endpoint's capability limits.

## What a capture preserves

PageKnot captures the browser state that produced the visible page:

- Rendered HTML after page scripts have run
- Stylesheets, fonts, images, and responsive image selections
- Same-origin and cross-origin frames
- Open and observed closed shadow roots
- Canvas pixels and media poster frames
- Current form values, selected options, and disclosure state
- CSS Object Model rules and adopted stylesheets
- Source URL, browser environment, resource outcomes, and capture warnings

Captured page scripts are removed from the default artifact. The saved document
contains the rendered result and a content security policy that blocks external
connections.

## A capture is a transaction

PageKnot treats the requested output path as a commit boundary.

1. It validates the request and destination.
2. It collects the page into bounded temporary storage.
3. It resolves and embeds render-affecting resources.
4. It writes a staging artifact beside the destination.
5. It verifies the artifact with network access blocked.
6. It atomically commits the verified file.

A failed or cancelled job leaves the requested destination unchanged. The
capture report names each failed, omitted, external, and embedded resource.

## Inspect and verify

Inspect the embedded manifest:

```console
pageknot inspect example.html
```

Read it as stable JSON:

```console
pageknot inspect example.html --json
```

Verify an artifact in a network-denied browser:

```console
pageknot verify example.html --level offline
```

Verification fails when a safe-static artifact attempts an external request,
contains an invalid manifest, references a missing embedded resource, or cannot
reach a stable browser state.

## Export a capture

Derive independently verified PDF, Markdown, ZIP, compressed HTML, or MHTML
artifacts from one verified capture:

```console
pageknot export example.html \
  --output exports \
  --variant pdf,markdown,zip,self-extracting,mhtml
```

Each exported format carries the source policy digest and resource summary.
PageKnot verifies every format before committing it.

## Run bounded capture sets

Capture a breadth-first, same-origin link graph with explicit limits:

```console
pageknot crawl https://example.com \
  --output captures \
  --max-pages 100 \
  --max-depth 3 \
  --concurrency 4 \
  --resume capture-state.json
```

Use `pageknot batch jobs.json` for independent capture requests. The
[`BatchRequest`](./schemas/batch-request.schema.json) schema defines the
manifest contract. Batch and crawl results preserve one terminal outcome per
scheduled page and can resume from their atomic state files.

## Use PageKnot in automation

`--json` writes one versioned result object to stdout. Logs and progress remain
on stderr.

```console
pageknot capture https://example.com \
  --output example.html \
  --json > capture.json

jq -e '
  .status == "succeeded" and
  .verification.passed and
  .verification.networkRequests == 0
' capture.json
```

Every result includes redacted requested and final URLs, source URL digests,
artifact digest, byte count, browser version, policy digest, resource summary,
warnings, and stage timings.

## Configure repeatable capture profiles

Store advanced behavior in a named profile:

```toml
[profile.research]
verification = "offline"
missing_resources = "fail"

[profile.research.environment]
viewport = { width = 1440, height = 900, scale = 1 }
locale = "en-US"
timezone = "UTC"
color_scheme = "light"

[profile.research.readiness]
mode = "network-idle"
network_quiet = "750ms"
delay = "1s"

[profile.research.limits]
duration = "2m"
resource_bytes = "64MiB"
total_resource_bytes = "512MiB"
frames = 256
```

Apply the profile from the CLI:

```console
pageknot capture https://example.com --profile research
```

Flags override environment variables, which override the selected profile and
user configuration.

## Rust

The CLI calls the same service API exposed by the `pageknot` crate:

```rust
use pageknot::PageKnot;

#[tokio::main]
async fn main() -> pageknot::Result<()> {
    let pageknot = PageKnot::builder().build()?;

    let result = pageknot
        .capture("https://example.com")?
        .save("example.html")
        .await?;

    println!("{}", result.artifact.path().display());
    pageknot.close().await?;
    Ok(())
}
```

Use `CaptureBuilder::wait_until` and `CaptureBuilder::delay` to apply the same
readiness condition and post-condition delay from Rust.

Long-running applications can start a `CaptureJob`, consume typed progress
events, cancel it by ID, and await the same `CaptureResult` returned by the
one-shot API.

## Node.js

```console
npm install @pageknot/node
```

```ts
import { PageKnot } from "@pageknot/node";

const pageknot = new PageKnot();

try {
  const result = await pageknot.capture("https://example.com", {
    output: "example.html",
  });

  console.log(result.artifact.path);
} finally {
  await pageknot.close();
}
```

Set `waitUntil: "network-idle"` and `delayMs: 1000` to apply the same
readiness behavior from Node.js.

## Python

```console
pip install pageknot
```

```python
import asyncio

from pageknot import PageKnot


async def main() -> None:
    async with PageKnot() as pageknot:
        result = await pageknot.capture(
            "https://example.com",
            output="example.html",
        )
        print(result.artifact.path)


asyncio.run(main())
```

Set `wait_until="network-idle"` and `delay_ms=1000` to apply the same
readiness behavior from Python.

Rust, Node.js, Python, and CLI callers share the same defaults, error codes,
capture reports, and artifact format.

## The PageKnot promise

**Saved means self-contained.** A successful safe-static capture reopens with
network access blocked.

**Fidelity starts from observation.** PageKnot captures the browser's rendered
state, including state that is absent from the original HTML response.

**Every gap is visible.** Resource failures and unsupported browser state are
records in the result, never silent omissions.

**Automation is a primary interface.** stdout, stderr, JSON, exit statuses,
cancellation, timeouts, and overwrite behavior are stable contracts.

**The engine belongs outside the command parser.** The CLI and language SDKs
call the same services and receive the same domain records.

**Artifacts carry provenance.** Each file records its source, capture policy,
browser environment, resource summary, and PageKnot format version.

**Local capture stays local.** PageKnot has no hosted capture dependency and
collects no telemetry by default.

## Design and implementation

[`SPEC.md`](./SPEC.md) defines the product contract, service API, workspace,
capture pipeline, security model, language bindings, test matrix, release
gates, and implementation sequence.

[`docs/public-api.md`](./docs/public-api.md) records the Rust compatibility
surface. [`docs/threat-model.md`](./docs/threat-model.md) defines the security
boundaries. [`docs/performance-baseline.md`](./docs/performance-baseline.md)
records the repeated-capture reference run.
