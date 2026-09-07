<p align="center">
  <a href="./docs/index.md">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/public/brand/offprint-lockup-horizontal-dark.svg">
      <img alt="Offprint" src="docs/public/brand/offprint-lockup-horizontal-light.svg" width="360">
    </picture>
  </a>
</p>

**Save a rendered web page as self-contained HTML you can reopen offline.**

Offprint captures the page in [Chromium](https://www.chromium.org/Home/),
embeds its images, styles, fonts, and current browser state, then verifies the
saved page with networking blocked. Its manifest records the source and
resource outcomes, and the capture receipt reports verification evidence.

Offprint is alpha. APIs and artifact formats can change.

## Capture a page

Build from this checkout with [Rust](https://www.rust-lang.org/tools/install):

```console
cargo build --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-cli
./offprint-rs/target/release/offprint capture https://example.com --output example.html --quiet
```

The command prints `example.html`. Open that file in a browser, or inspect its
manifest:

```console
./offprint-rs/target/release/offprint artifact inspect example.html
```

Offprint uses an installed Chrome, Chromium, or Microsoft Edge. If none is
available, it downloads and verifies a pinned browser on first use. Existing
output files are preserved unless you select `--on-exists replace`.

Capturing executes the source page and can save private content. Captured page
scripts are stripped from the artifact. Read the
[security guide](./docs/operations/security.md) before capturing untrusted or
authenticated pages.

- **Keep the rendered state.** Capture dynamic content, forms, frames, shadow
  roots, and canvas pixels.
- **Check what was saved.** Inspect resource records and warnings, or fail the
  capture when a resource cannot be embedded.
- **Write after verification.** Save one HTML file, return bounded bytes, or
  export PDF, Markdown, ZIP, self-extracting HTML, and MHTML.
- **Build capture into your tools.** Use the CLI, Rust, Node.js, or Python with
  shared capture, cancellation, and verification behavior.

## Documentation

[Install](./docs/start/install.md) ·
[Quickstart](./docs/start/quickstart.md) ·
[Documentation](./docs/index.md)

- [Control readiness and page selection](./docs/guides/control-capture.md)
- [Capture authenticated pages](./docs/guides/authenticated-pages.md)
- [Run batches and crawls](./docs/guides/batch-and-crawl.md)
- [Save documentation as PDF](./docs/guides/export-pdf.md)
- [Inspect, verify, and export](./docs/guides/inspect-verify-export.md)
- [Use Rust](./docs/integrations/rust.md),
  [Node.js](./docs/integrations/node.md), or
  [Python](./docs/integrations/python.md)

## Development

Read [contributor setup](./development_docs/setup.md) for the local check loop
and [architecture](./development_docs/architecture.md) for ownership and
extension boundaries. [Development docs](./development_docs/README.md) route
changes to their source, tests, and generation commands.

Licensed under [AGPL-3.0-or-later](./LICENSE).
