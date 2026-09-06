# Offprint

**Capture a rendered web page as verified, self-contained HTML.**

Offprint opens a URL in
[Chromium](https://www.chromium.org/Home/), waits for the page's rendering boundary,
captures browser state and render dependencies, removes captured page code,
verifies the staged artifact, and commits the requested file.

> **Status:** Offprint is an alpha project before its first tagged release.
> Public APIs, schemas, artifact formats, and package layout can change.

[Documentation](./docs/README.md) ·
[Quickstart](./docs/start/quickstart.md) ·
[Capture model](./docs/concepts/capture-model.md) ·
[Security](./docs/operations/security.md) ·
[Development](./development_docs/README.md)

## First capture

Build the native command-line interface from this checkout:

```console
cargo build --release --locked -p offprint-cli
```

Capture and offline-verify a page:

```console
./target/release/offprint capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path:

```text
example.html
```

Inspect the embedded artifact manifest, then repeat verification in a fresh
network-denied browser context:

```console
./target/release/offprint artifact inspect example.html
./target/release/offprint artifact verify example.html \
  --verification offline
```

Offprint uses a compatible Chrome, Chromium, or Microsoft Edge installation.
When discovery finds none, first use downloads the pinned Chrome for Testing
archive, verifies its digest, and stores it in the managed cache.

An existing output path fails by default. Use `--on-exists replace` to replace
it after verification, or `--on-exists uniquify` to choose a numbered path.

Offprint executes the source page and can save private rendered content. Treat
the URL, browser input, artifact, and diagnostics as untrusted.

## What the artifact records

Offprint captures rendered HTML, live stylesheet rules, frames, open and
observed closed shadow roots, current form and disclosure state, responsive
images, fonts, canvas pixels, media fallbacks, and render-affecting resources.

The canonical Offprint HTML artifact embeds a versioned manifest with redacted
source and source digests, browser, browser environment, capture-policy digest,
frame count, resource records, warning codes, and structural-repair state. Its
content security policy permits exact Offprint restoration programs and blocks
unlisted scripts and connections.

The capture receipt reports the file or memory delivery, artifact digest,
verification evidence, aggregate resource summary, warnings, and stage timings.
Individual resource records live in the artifact manifest.

## Choose the next path

| Goal | Documentation |
| --- | --- |
| Understand request, job, artifact, and receipt | [Capture model](./docs/concepts/capture-model.md) |
| Tune dynamic-page readiness or selection | [Control capture](./docs/guides/control-capture.md) |
| Capture authenticated content | [Authenticated pages](./docs/guides/authenticated-pages.md) |
| Run a batch or bounded crawl | [Batch and crawl](./docs/guides/batch-and-crawl.md) |
| Export PDF, Markdown, ZIP, self-extracting HTML, or MIME HTML (MHTML) | [Artifact formats](./docs/reference/formats.md) |
| Manage local or remote Chromium | [Browser concepts](./docs/concepts/browsers.md) |
| Automate with JSON and exit statuses | [Automation](./docs/guides/automation.md) |
| Diagnose a failure | [Troubleshooting](./docs/operations/troubleshooting.md) |

## Use the service API

The CLI and language bindings call the same Rust service.

| Interface | Start here |
| --- | --- |
| Rust | [Rust integration](./docs/integrations/rust.md) |
| Node.js | [Node.js integration](./docs/integrations/node.md) |
| Python | [Python integration](./docs/integrations/python.md) |

One `Offprint` service can reuse browser resources across captures. Use a
complete `CaptureRequest` for memory output, credentials, custom network rules,
diagnostics, and exact limits. Start a `CaptureJob` to consume events or request
cancellation. Await `Offprint::close`, `offprint.close()`, or the Python async
context manager during shutdown.

## Project contracts

- [`schemas/`](./schemas) contains generated request, event, result, error,
  browser, artifact, batch, crawl, and export contracts.
- [`development_docs/`](./development_docs/README.md) owns architecture,
  lifecycle, code generation, validation, dependencies, and release workflows.
- [`AGENTS.md`](./AGENTS.md) is the executable contributor contract.

Offprint is licensed under [AGPL-3.0-or-later](./LICENSE).
