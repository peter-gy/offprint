# Offprint CLI

The `offprint` command captures rendered pages, verifies artifacts, derives
additional formats, and manages its Chromium runtime. Commands write data to
stdout and progress or diagnostics to stderr.

Build the executable from the repository root:

```console
cargo build --release --locked -p offprint-cli
```

The examples below use `offprint` as the executable name. From the source
checkout, substitute `./target/release/offprint`.

## Capture and verify a page

```console
offprint capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path:

```text
example.html
```

Offprint fails when that path already exists. Pass `--on-exists replace` to
replace it after the staged artifact passes verification. A failed or cancelled
capture leaves the existing file unchanged.

Verify the committed artifact again:

```console
offprint artifact verify example.html --verification offline
```

Offline verification reopens HTML with network access denied. A successful
verification reports `0 network requests`.

## Commands

| Command | Result and side effect |
| --- | --- |
| `capture <URL>` | Writes one verified Offprint HTML artifact to the required output. |
| `artifact export <ARTIFACT>` | Derives selected PDF, Markdown, ZIP, self-extracting HTML, or MHTML artifacts from an Offprint HTML artifact. |
| `batch <MANIFEST>` | Runs the independent requests in a [`BatchRequest`](../schemas/batch-request.schema.json) JSON document. |
| `crawl <URL>` | Captures a breadth-first link graph into a directory. Defaults are 100 pages, depth 3, concurrency 4, and seed-origin links. |
| `artifact verify <ARTIFACT>` | Applies static checks or a network-denied browser reopen. |
| `artifact inspect <ARTIFACT>` | Validates and prints the embedded HTML artifact manifest. |
| `doctor` | Reports effective configuration, browser selection, collector compatibility, cache state, and output capabilities. |
| `browser install` | Downloads and verifies a trusted managed Chromium revision. |
| `browser list` | Lists managed and discovered browser candidates. |
| `browser remove <REVISION>` | Removes a selected idle managed revision. `--force` first resolves a replacement. |
| `completion <SHELL>` | Prints a completion script for Bash, Elvish, Fish, PowerShell, or Zsh. |

Run `offprint <command> --help` for the current arguments, accepted values, and
examples.

## Export another format

`capture` commits canonical safe-static HTML. Derive a PDF from that capture
artifact:

```console
offprint capture https://example.com \
  --output example.html

offprint artifact export example.html \
  --output exports \
  --format pdf
```

PDF output preserves Chromium's selectable text, printable links, tagged
structure, and document outline. Offprint adds the HTML title, language,
author, description, keywords, source URL, capture time, and provenance to
document properties and Extensible Metadata Platform metadata. It verifies the
passive PDF structure before commit.

Use `--landscape` or `--prefer-css-page-size` on `artifact export` to control PDF
printing.

Derive several independently verified formats from an existing Offprint HTML
artifact:

```console
offprint artifact export example.html \
  --output exports \
  --format pdf,markdown,zip,self-extracting-html,mhtml
```

Each derived artifact carries source provenance and passes its format-specific
verifier before Offprint commits the output set. Export fails when a destination
already exists. Pass `--on-exists replace` to replace verified artifacts.

## Select capture readiness

`capture` waits for `render-idle` by default. Choose a narrower browser
milestone when the page has a known lifecycle:

| Value | Capture begins after |
| --- | --- |
| `render-idle` | Load, lazy loading, fonts, Document Object Model quiet, and network quiet |
| `network-idle` | Document Object Model content loaded, then zero finite requests for the configured quiet window |
| `load` | The browser load milestone |
| `dom-content-loaded` | The browser Document Object Model content loaded milestone |

`network-idle` excludes WebSocket, EventSource, `blob:`, and `data:` lifetimes.
Use `render-idle` when fetch or XMLHttpRequest calls stay open. Add `--delay`
when worker computation updates the page after the chosen condition:

```console
offprint capture https://example.com \
  --wait-until network-idle \
  --delay 1s \
  --timeout 2m \
  --output application.html
```

Readiness and delay share the deadline set by `--timeout`.

## Capture one element

`--selector` keeps the document head, the first matching element, and its
ancestor chain:

```console
offprint capture \
  https://www.datawrapper.de/blog/dual-axis-charts-guide \
  --selector "main article" \
  --output article.html
```

Invalid CSS syntax returns `offprint.selector.invalid`. A valid selector with
no top-level document match returns `offprint.selector.not_found`.

## Write machine-readable output

`--json` writes one versioned result object to stdout. Redirect it without
capturing progress text:

```console
offprint capture https://example.com \
  --output example.html \
  --json > capture.json

jq -e '
  (.captureId | startswith("cap_")) and
  .verification.networkRequests == 0
' capture.json
```

`--quiet` suppresses non-error human diagnostics. `--output -` writes raw
artifact bytes to stdout and conflicts with `--json`.

`artifact verify --json` always returns one `ArtifactVerification` record. Its
`format` and `method` fields identify HTML static checks, HTML offline checks,
or a format-specific verifier.

## Run bounded capture sets

Use `batch` for independent requests defined by a
[`BatchRequest`](../schemas/batch-request.schema.json) JSON document:

```console
offprint batch jobs.json
```

Persisted capture, batch, crawl, and export requests require `schemaVersion`.
Unknown fields and incompatible versions fail before browser or filesystem
work begins.

Use `crawl` for a breadth-first link graph with explicit limits:

```console
offprint crawl https://example.com \
  --output captures \
  --max-pages 100 \
  --max-depth 3 \
  --concurrency 4 \
  --resume capture-state.json
```

The crawl stays on the seed origin unless `--allow-cross-origin` is set. Batch
and crawl state preserve one terminal outcome per page and support resumable
runs.

## Exit statuses

| Status | Meaning |
| ---: | --- |
| `0` | The command completed successfully |
| `1` | A runtime operation failed |
| `2` | Input or configuration was invalid |
| `3` | Artifact verification failed |
| `130` | The command was interrupted |

Structured failures carry a stable `offprint.*` code, pipeline stage,
retryability, details, and an optional diagnostics path. The canonical catalog
is [`schemas/error-codes.json`](../schemas/error-codes.json).

In JSON mode, a failure writes the complete `OffprintError` record to stderr.
Human mode writes a concise error line to stderr. Success data remains on
stdout in both modes.

## Use credentials

Pass request headers or browser cookies through a protected JSON file:

```console
chmod 600 headers.json
offprint capture https://example.com/account \
  --headers headers.json \
  --output account.html
```

The header file can be a JSON object:

```json
{
  "Authorization": "Bearer token"
}
```

Credential files must be regular files no larger than 1 MiB. On Unix, group
and other permissions must be clear. Offprint validates Windows access control
entries before reading a file. `--headers -` or `--cookies -` reads one
credential input from stdin.

The artifact can contain authenticated page content. Store and share it with
the same controls as the source data. See [Configuration](./configuration.md)
for cookie shape and network policies.

## Generate shell completion

Write the generated script to the location used by the shell:

```console
offprint completion zsh > _offprint
```

The command prints the script and does not edit shell configuration.

## Next steps

- Use [configuration profiles](./configuration.md) for repeatable captures.
- Follow [troubleshooting](./troubleshooting.md) when setup, readiness, or
  verification fails.
- Review the [security threat model](./threat-model.md) before capturing
  untrusted or authenticated pages.
