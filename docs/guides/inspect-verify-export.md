# Inspect, verify, and export artifacts

Offprint HTML is the source for inspection, repeated verification, and export.
Use the operation that matches the evidence you need.

## Inspect the artifact manifest

```console
offprint artifact inspect example.html --json > manifest.json
```

Inspection validates the embedded artifact manifest and returns it. It does not
check the rest of the HTML artifact. Use it to read provenance, resource
records, warning codes, browser environment, or the requested verification
mode. Full warning records belong to the capture receipt.

## Run static verification

```console
offprint artifact verify example.html \
  --verification static \
  --json > verification.json
```

Static verification parses the complete artifact. It checks the manifest,
content security policy, exact Offprint-owned scripts, frame accounting,
resource records, embedded syntax and digests, structural repair, and forbidden
request references. It does not launch a browser.

## Run offline verification

```console
offprint artifact verify example.html \
  --verification offline \
  --json > verification.json
```

Offline verification includes static checks and opens the artifact in a fresh
network-denied Chromium context. It waits for images, fonts, frames, and page
stability, then rejects any attempted request or page error.

Artifact input from standard input is limited to 64 MiB. Offline verification
copies stream input into bounded temporary storage before opening it.

## Export alternate representations

For page selection, print layout, and PDF checks, follow
[Save a web page as PDF](./export-pdf.md).

```console
offprint artifact export example.html \
  --output exports \
  --format pdf,markdown,zip,self-extracting-html,mhtml \
  --json > export-result.json
```

Export first obtains offline evidence for the source HTML. It then encodes and
verifies every requested representation before committing the output set.
Duplicate formats and an empty format list are rejected.

For base name `example`, the default paths are:

| Format               | Output path                       | Entrypoint                          |
| -------------------- | --------------------------------- | ----------------------------------- |
| PDF                  | `exports/example.pdf`             | `exports/example.pdf`               |
| Markdown             | `exports/example-markdown/`       | `exports/example-markdown/index.md` |
| ZIP                  | `exports/example.zip`             | `exports/example.zip`               |
| Self-extracting HTML | `exports/example.compressed.html` | `exports/example.compressed.html`   |
| MHTML                | `exports/example.mhtml`           | `exports/example.mhtml`             |

The export result distinguishes `path`, which can be a file or directory, from
`entrypoint`, which is the item a user opens.

## Verify one exported format

```console
offprint artifact verify exports/example.pdf --format pdf --json
```

Format-specific verification requires a filesystem path. It verifies the
representation's format-owned structure and content contract. Provenance checks
differ by format. It does not perform HTML offline verification and cannot use
HTML verification options.

## Recover an interrupted export

An empty output directory can commit through a directory swap. A nonempty
directory uses a journaled, recoverable set commit. After source verification
and format encoding, Offprint creates the output directory when needed. The
transaction then validates its destination set, records installed entries, and
rolls back after a failure when possible.

An output error can include `transactionPath`, `commitComplete`,
`partialCommit`, `recoveryPending`, or `recoveryPaths`. Preserve those paths
until recovery completes. Retrying the same output directory lets Offprint
resolve a recoverable journal before starting new work.

Choose representation behavior through the [format reference](../reference/formats.md).
