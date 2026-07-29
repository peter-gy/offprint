# PageKnot CLI

The `pageknot` command captures rendered pages, verifies artifacts, derives
additional formats, and manages its Chromium runtime. Commands write data to
stdout and progress or diagnostics to stderr.

Build the executable from the repository root:

```console
cargo build --release --locked -p pageknot-cli
```

The examples below use `pageknot` as the executable name. From the source
checkout, substitute `./target/release/pageknot`.

## Capture and verify a page

```console
pageknot capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path:

```text
example.html
```

PageKnot replaces an existing file at that path after the staged artifact
passes verification. A failed or cancelled capture leaves the existing file
unchanged.

Verify the committed artifact again:

```console
pageknot verify example.html --level offline
```

Offline verification reopens HTML with network access denied. A successful
verification reports `0 network requests`.

## Commands

| Command | Result and side effect |
| --- | --- |
| `capture <URL>` | Writes one verified HTML or PDF artifact. The default format is HTML. |
| `export <ARTIFACT>` | Derives selected PDF, Markdown, ZIP, compressed HTML, or MHTML variants from a PageKnot HTML artifact. Existing variants are replaced by default. |
| `batch <MANIFEST>` | Runs the independent requests in a [`BatchRequest`](../schemas/batch-request.schema.json) JSON document. |
| `crawl <URL>` | Captures a breadth-first link graph into a directory. Defaults are 100 pages, depth 3, concurrency 4, and seed-origin links. |
| `verify <ARTIFACT>` | Applies static checks or a network-denied browser reopen. |
| `inspect <ARTIFACT>` | Validates and prints the embedded HTML artifact manifest. |
| `doctor` | Reports effective configuration, browser selection, collector compatibility, cache state, and output capabilities. |
| `browser install` | Downloads and verifies a trusted managed Chromium revision. |
| `browser list` | Lists managed and discovered browser candidates. |
| `browser remove <REVISION>` | Removes a selected idle managed revision. `--force` first resolves a replacement. |
| `completion <SHELL>` | Prints a completion script for Bash, Elvish, Fish, PowerShell, or Zsh. |

Run `pageknot <command> --help` for the current arguments, accepted values, and
examples.

## Choose a representation

`capture` commits safe-static HTML by default. Pass `--format pdf` to render
the captured HTML as the committed PDF:

```console
pageknot capture https://example.com \
  --format pdf \
  --output example.pdf
```

PDF output preserves Chromium's selectable text, printable links, tagged
structure, and document outline. PageKnot adds the HTML title, language,
author, description, keywords, source URL, capture time, and provenance to
document properties and Extensible Metadata Platform metadata. It verifies the
passive PDF structure before commit.

Use `--landscape` or `--prefer-css-page-size` to control PDF printing.

Derive several independently verified formats from an existing PageKnot HTML
artifact:

```console
pageknot export example.html \
  --output exports \
  --variant pdf,markdown,zip,self-extracting,mhtml
```

Each derived artifact carries source provenance and passes its format-specific
verifier before PageKnot commits the output set. Existing export destinations
are replaced by default.

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
pageknot capture https://example.com \
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
pageknot capture \
  https://www.datawrapper.de/blog/dual-axis-charts-guide \
  --selector "main article" \
  --output article.html
```

Invalid CSS syntax returns `pageknot.selector.invalid`. A valid selector with
no top-level document match returns `pageknot.selector.not_found`.

## Write machine-readable output

`--json` writes one versioned result object to stdout. Redirect it without
capturing progress text:

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

`--quiet` suppresses non-error human diagnostics. `--output -` writes raw
artifact bytes to stdout and conflicts with `--json`.

## Run bounded capture sets

Use `batch` for independent requests defined by a
[`BatchRequest`](../schemas/batch-request.schema.json) JSON document:

```console
pageknot batch jobs.json
```

Use `crawl` for a breadth-first link graph with explicit limits:

```console
pageknot crawl https://example.com \
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

Structured failures carry a stable `pageknot.*` code, pipeline stage,
retryability, details, and an optional diagnostics path. The canonical catalog
is [`schemas/error-codes.json`](../schemas/error-codes.json).

## Use credentials

Pass request headers or browser cookies through a protected JSON file:

```console
chmod 600 headers.json
pageknot capture https://example.com/account \
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
and other permissions must be clear. PageKnot validates Windows access control
entries before reading a file. `--headers -` or `--cookies -` reads one
credential input from stdin.

The artifact can contain authenticated page content. Store and share it with
the same controls as the source data. See [Configuration](./configuration.md)
for cookie shape and network policies.

## Generate shell completion

Write the generated script to the location used by the shell:

```console
pageknot completion zsh > _pageknot
```

The command prints the script and does not edit shell configuration.

## Next steps

- Use [configuration profiles](./configuration.md) for repeatable captures.
- Follow [troubleshooting](./troubleshooting.md) when setup, readiness, or
  verification fails.
- Review the [security threat model](./threat-model.md) before capturing
  untrusted or authenticated pages.
