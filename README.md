<p align="center">
  <a href="https://peter-gy.github.io/offprint/">
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

Use [`uvx`](https://docs.astral.sh/uv/guides/tools/), uv's Python command runner:

```console
uvx offprint capture https://example.com --output example.html --quiet
uvx offprint artifact inspect example.html
```

Or use [`npx`](https://docs.npmjs.com/cli/commands/npx), npm's command runner,
with Node.js 22 or newer:

```console
npx offprint capture https://example.com --output example.html --quiet
npx offprint artifact inspect example.html
```

The capture command prints `example.html`. Open the file in a browser.
`artifact inspect` reads its embedded manifest.

Offprint uses an installed Chrome, Chromium, or Microsoft Edge. If none is
available, it downloads and verifies a pinned browser on first use. Existing
output files are preserved unless you select `--on-exists replace`.

Capturing executes the source page and can save private content. Captured page
scripts are stripped from the artifact. Read the
[security guide](https://peter-gy.github.io/offprint/operations/security.html) before capturing untrusted or
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

[Install](https://peter-gy.github.io/offprint/start/install.html) ·
[Quickstart](https://peter-gy.github.io/offprint/start/quickstart.html) ·
[Documentation](https://peter-gy.github.io/offprint/)

- [Control readiness and page selection](https://peter-gy.github.io/offprint/guides/control-capture.html)
- [Capture authenticated pages](https://peter-gy.github.io/offprint/guides/authenticated-pages.html)
- [Run batches and crawls](https://peter-gy.github.io/offprint/guides/batch-and-crawl.html)
- [Save documentation as PDF](https://peter-gy.github.io/offprint/guides/export-pdf.html)
- [Inspect, verify, and export](https://peter-gy.github.io/offprint/guides/inspect-verify-export.html)
- [Use Rust](https://peter-gy.github.io/offprint/integrations/rust.html),
  [Node.js](https://peter-gy.github.io/offprint/integrations/node.html), or
  [Python](https://peter-gy.github.io/offprint/integrations/python.html)

## Development

Read [contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md) for the local check loop
and [architecture](https://github.com/peter-gy/offprint/blob/main/development_docs/architecture.md) for ownership and
extension boundaries. [Development docs](https://github.com/peter-gy/offprint/blob/main/development_docs/README.md) route
changes to their source, tests, and generation commands.

Licensed under [MIT](./LICENSE).
