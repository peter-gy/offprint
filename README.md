# Offprint

**Freeze a rendered web page into verified, self-contained HTML.**

Offprint opens a URL in headless Chromium, waits for the page to settle,
collects the rendered document and its resources, removes active page code,
reopens the staged artifact with network access denied, and commits the verified
file. PDF, Markdown, ZIP, self-extracting HTML, and MHTML are exports derived
from that canonical capture artifact.

> **Status:** Offprint is an alpha project before its first tagged release.
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
cargo build --release --locked -p offprint-cli
```

Offprint uses a compatible Chrome or Chromium installation when one is
available. Otherwise, the first capture downloads its pinned Chrome for Testing
build, verifies the archive, and stores the browser in the managed cache.

Capture a page:

```console
./target/release/offprint capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path to stdout:

```text
example.html
```

Open `example.html` in a browser, or repeat the offline verification:

```console
./target/release/offprint artifact verify example.html --verification offline
```

A successful verification reports `0 network requests`. Verification proves
self-containment and structural policy compliance. It does not certify the
truth or safety of the captured page.

An explicit output path fails when the destination already exists. Pass
`--on-exists replace` to replace it after the staged artifact passes
verification. A failed or cancelled capture leaves the existing file unchanged.

Offprint executes page scripts and fetches page resources inside an isolated
browser context. Treat the URL, page, and resulting artifact as untrusted
input.

The guides use `offprint` as the executable name. From a source checkout, run
`./target/release/offprint` in its place.

## What an HTML capture preserves

Offprint captures the browser state that produced the visible page:

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
source, capture policy, browser, resource outcomes, warnings, format version,
and requested verification mode. The capture receipt contains the verification
report produced after the artifact bytes are complete.

## Export another format

Offprint HTML is the source for inspection, offline verification, and export.
Capture it first, then derive a PDF:

```console
offprint capture https://example.com \
  --output example.html

offprint artifact export example.html \
  --output exports \
  --format pdf
```

PDF output preserves selectable text, printable links, tagged structure,
document outline, and source metadata. The
[CLI guide](./docs/cli.md#export-another-format) covers print options and every
supported artifact format.

## How a capture reaches the output path

Offprint treats the requested output path as a commit boundary:

1. Validate the request and destination.
2. Collect the rendered page into bounded temporary storage.
3. Resolve and embed render-affecting resources.
4. Write a staging artifact beside the destination.
5. Run the selected static or offline verification policy.
6. Atomically commit the verified file.

The capture receipt records every embedded, failed, omitted, and external
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

The installed executable is the exact flag reference. Run `offprint --help` or
`offprint <command> --help` for the current command surface.

## Use Offprint as a service

The CLI and language bindings call the same Rust service and share capture
records, defaults, errors, and artifact formats.

| Interface | Start here |
| --- | --- |
| CLI | [CLI guide](./docs/cli.md) |
| Rust | [Rust public API](./docs/public-api.md) |
| Node.js | [`offprint`](./bindings/node/README.md) |
| Python | [`offprint`](./bindings/python/README.md) |

Long-running callers can share one `Offprint` service, start typed
`CaptureJob` values, consume progress events, cancel through the job handle, and close the
service to release owned browser processes.

## Project references

- [Documentation](./docs/README.md) routes user, API, and maintainer tasks.
- [Concepts](./docs/concepts.md) defines the product vocabulary and lifecycle.
- [Feature and parity matrix](./docs/feature-matrix.md) maps capabilities to
  implementation and release evidence.
- [`schemas/`](./schemas) contains versioned request, result, error, and binding
  contracts.
- [`SPEC.md`](./SPEC.md) defines the product contract, capture pipeline,
  workspace, and release gates.

Offprint is licensed under
[`AGPL-3.0-or-later`](./LICENSE).
