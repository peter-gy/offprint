# What is Offprint?

Offprint saves the page a browser rendered as a self-contained HTML file. Use
it to preserve a report, article, dashboard, or other page for later viewing,
review, or processing.

After [installing the CLI](./install.md):

```console
offprint capture https://example.com --output example.html
```

Offprint opens the URL in [Chromium](https://www.chromium.org/Home/), the browser
engine used by Chrome. It waits for rendered content, embeds rendering
resources, freezes page state, verifies the HTML, and commits `example.html`.
You can open that file from disk with network access disabled.

## What a capture preserves

| Page content | Saved representation |
| --- | --- |
| Text and elements created by scripts | Rendered HTML |
| Stylesheets, fonts, images, and SVG | Embedded rendering resources |
| Nested frames and shadow roots | Captured documents and shadow content |
| Form controls, disclosure elements, and scroll positions | State observed at capture time |
| Canvas and media | Pixels, posters, or current-frame fallbacks |
| Source and capture conditions | Embedded artifact manifest |

Password input values are redacted by default. Resource failures can reduce
fidelity and appear as warnings. Choose `--missing-resources fail` when every
discovered resource must be embedded.

## What verification establishes

An **Offprint HTML artifact** is the saved page and its embedded manifest.
Offprint calls its execution and network contract **safe-static HTML**:
captured page scripts and event handlers are stripped, rendering dependencies
are embedded, and a
[Content Security Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/CSP)
restricts script execution and network access. The policy permits exact
Offprint programs needed to restore captured scroll or structural state.

By default, Offprint checks the artifact and reopens it in a fresh browser
context with network access denied. The file is committed after verification
succeeds. Verification establishes self-containment and format validity. It
does not certify the source's truth or make private content safe to share.

A **capture receipt** reports the saved path, digest, verification evidence,
resource counts, warnings, and timings. The CLI prints it with `--json`.
Language APIs return it directly.

## Capture once, use the result

Inspect the embedded manifest, repeat verification, or create PDF, Markdown,
ZIP, self-extracting HTML, and MHTML exports from the saved HTML. Choose an
[export format](../reference/formats.md) for the reader or application that will
consume it. Exports preserve different subsets of the captured page.

Use a batch for known URLs or a bounded crawl to discover links. Each page
produces its own capture result. Use the [CLI](../reference/cli.md) for shell
work or integrate the shared service through [Rust](../integrations/rust.md),
[Node.js](../integrations/node.md), or [Python](../integrations/python.md).

Start with [one capture](./quickstart.md). Read
[why Offprint verifies captured pages](./why-offprint.md) for the design choices
behind the result.
