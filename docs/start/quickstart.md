# Capture and verify one page

Save a page, open the HTML file, and inspect the evidence that travels with it.
[Install the CLI](./install.md#build-the-cli-from-source) before starting.

Offprint needs network access to the page. It uses a compatible local browser
or downloads and verifies the pinned
[Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)
build on first use. Chrome for Testing is Google's versioned Chromium browser
distribution.

## Capture the page

```console
offprint capture https://example.com --output example.html --quiet
```

The command prints the saved path:

```text
example.html
```

Open `example.html` in your browser. The file contains the rendered page and its
rendering resources. Offprint already reopened it with network access denied
before committing the file. Offline verification is the default.

A capture runs the source page's code inside an isolated browser context. The
saved HTML retains captured content and state, while captured page scripts are
stripped. Captured private content remains private data.

## Repeat a capture

By default, an existing destination produces an error. Replace it after the new
capture verifies successfully:

```console
offprint capture https://example.com \
  --output example.html \
  --on-exists replace
```

Use `--on-exists uniquify` to keep both captures under distinct file names.

## Read the saved evidence

```console
offprint artifact inspect example.html
```

Inspection reads the embedded **artifact manifest**: source, capture time,
browser, resource counts, warning codes, and requested verification mode. It
validates the manifest. To check the complete file again, run:

```console
offprint artifact verify example.html
```

Verification checks the HTML structure and embedded resources, then reopens the
file in a fresh browser context with network access denied. A successful run
reports zero observed network requests.

A self-contained artifact can still have missing content. The default resource
policy records warnings and substitutes inert fallbacks when an image, font,
or other resource cannot be captured. Use `--missing-resources fail` when a
missing resource must fail the capture.

## Continue

- [Wait for content or select part of a page](../guides/control-capture.md)
- [Capture authenticated pages](../guides/authenticated-pages.md)
- [Create PDF, Markdown, and other exports](../guides/inspect-verify-export.md)
- [Read JSON results in automation](../guides/automation.md)
- [Diagnose browser or capture failures](../operations/troubleshooting.md)
