# PageKnot

**Capture a rendered web page as a verified, self-contained HTML or PDF file.**

PageKnot opens a URL in headless Chromium, waits for the page to settle,
collects the rendered document and its resources, and commits one portable
artifact. The default HTML workflow reopens the staged file with network access
denied before it reaches the requested output path.

> **Status:** PageKnot is an alpha project before its first tagged release.
> Build it from this repository. Public APIs, JSON schemas, and artifact formats
> may change.

[Documentation](./docs/README.md) ·
[CLI guide](./docs/cli.md) ·
[Configuration](./docs/configuration.md) ·
[Security model](./docs/threat-model.md) ·
[Rust API](./docs/public-api.md)

## Quickstart

The repository selects Rust 1.97 through `rust-toolchain.toml`. Build the native
CLI:

```console
cargo build --release --locked -p pageknot-cli
```

PageKnot uses a compatible Chrome or Chromium installation when one is
available. Otherwise, the first capture downloads its pinned Chrome for Testing
build, verifies the archive, and stores the browser in the managed cache.

Capture a page:

```console
./target/release/pageknot capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path to stdout:

```text
example.html
```

Open `example.html` in a browser, or repeat the offline verification:

```console
./target/release/pageknot verify example.html --level offline
```

A successful verification reports `0 network requests`. Verification proves
self-containment and structural policy compliance. It does not certify the
truth or safety of the captured page.

File outputs replace an existing destination after the staged artifact passes
verification. A failed or cancelled capture leaves the existing file
unchanged.

PageKnot executes page scripts and fetches page resources inside an isolated
browser context. Treat the URL, page, and resulting artifact as untrusted
input.

The guides use `pageknot` as the executable name. From a source checkout, run
`./target/release/pageknot` in its place.

## What an HTML capture preserves

PageKnot captures the browser state that produced the visible page:

- Rendered HTML after page scripts have run
- Stylesheets, fonts, images, and responsive image selections
- Same-origin and cross-origin frames
- Open and observed closed shadow roots
- Canvas pixels and media poster frames
- Current form values, selected options, and disclosure state
- CSS Object Model rules and adopted stylesheets
- Source URL, browser environment, resource outcomes, and capture warnings

The saved document removes captured page scripts and carries a content security
policy that blocks external connections. Its embedded manifest records the
source, capture policy, browser, resource outcomes, warnings, and verification
result.

## Choose a representation

HTML is the source representation for inspection, offline verification, and
later export. Pass `--format pdf` when the committed result should be a PDF:

```console
pageknot capture https://example.com \
  --format pdf \
  --output example.pdf
```

PDF output preserves selectable text, printable links, tagged structure,
document outline, and source metadata. The
[CLI guide](./docs/cli.md#choose-a-representation) covers print options and
formats derived from an existing HTML capture.

## How a capture reaches the output path

PageKnot treats the requested output path as a commit boundary:

1. Validate the request and destination.
2. Collect the rendered page into bounded temporary storage.
3. Resolve and embed render-affecting resources.
4. Write a staging artifact beside the destination.
5. Run the selected static or offline verification policy.
6. Atomically commit the verified file.

The capture result records every embedded, failed, omitted, and external
resource. Use `--missing-resources fail` when an unresolved resource should
fail the capture.

## Find the next task

| Goal | Guide |
| --- | --- |
| Capture one matching element | [Selector capture](./docs/cli.md#capture-one-element) |
| Wait for an application to finish rendering | [Capture readiness](./docs/cli.md#select-capture-readiness) |
| Inspect, verify, or automate a capture | [CLI workflows](./docs/cli.md) |
| Capture a batch or bounded link graph | [Batch and crawl](./docs/cli.md#run-bounded-capture-sets) |
| Reuse settings across captures | [Configuration profiles](./docs/configuration.md) |
| Select a browser or remote CDP endpoint | [Browser settings](./docs/configuration.md#browser-settings) |
| Recover from a failed command | [Troubleshooting](./docs/troubleshooting.md) |
| Review network, credential, and artifact boundaries | [Security threat model](./docs/threat-model.md) |

The installed executable is the exact flag reference. Run `pageknot --help` or
`pageknot <command> --help` for the current command surface.

## Use PageKnot as a service

The CLI and language bindings call the same Rust service and share capture
records, defaults, errors, and artifact formats.

| Interface | Start here |
| --- | --- |
| CLI | [CLI guide](./docs/cli.md) |
| Rust | [Rust public API](./docs/public-api.md) |
| Node.js | [`@pageknot/node`](./bindings/node/README.md) |
| Python | [`pageknot`](./bindings/python/README.md) |

Long-running callers can share one `PageKnot` service, start typed
`CaptureJob` values, consume progress events, cancel by job ID, and close the
service to release owned browser processes.

## Project references

- [Documentation](./docs/README.md) routes user, API, and maintainer tasks.
- [Feature and parity matrix](./docs/feature-matrix.md) maps capabilities to
  implementation and release evidence.
- [`schemas/`](./schemas) contains versioned request, result, error, and binding
  contracts.
- [`SPEC.md`](./SPEC.md) defines the product contract, capture pipeline,
  workspace, and release gates.

PageKnot is licensed under
[`AGPL-3.0-or-later`](./LICENSE).
