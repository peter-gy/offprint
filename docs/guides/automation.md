# Automate Offprint

Offprint keeps command data on standard output and human diagnostics on
standard error. JSON mode uses versioned camelCase records shared by the CLI,
Node.js, and Python bindings.

The examples use [jq](https://jqlang.org/), a command-line JSON query tool, to
inspect stable fields.

## Capture one JSON result

```console
offprint capture https://example.com \
  --output example.html \
  --json > capture.json

jq -e '
  .schemaVersion == 2 and
  (.captureId | startswith("cap_")) and
  .artifact.kind == "file" and
  .verification.mode == "offline"
' capture.json
```

JSON mode suppresses capture progress so standard error remains reserved for a
structured failure. `--quiet` suppresses non-error human diagnostics in human
mode. Raw artifact bytes through `--output -` conflict with `--json`.

## Read failures without matching prose

An `OffprintError` contains:

- Stable `offprint.*` code
- Failure stage
- Redacted message
- Retryability
- Structured details
- Optional diagnostics path
- Optional nested source error

In JSON mode, the error record is written to standard error. Exit status
classifies the process result:

| Status | Meaning                                                                 |
| -----: | ----------------------------------------------------------------------- |
|    `0` | Operation succeeded                                                     |
|    `1` | Capture, browser, artifact input, output, scheduler, or runtime failure |
|    `2` | Arguments, configuration, or request validation failed                  |
|    `3` | Artifact verification rejected the input                                |
|  `130` | Operation was interrupted                                               |

Output conflicts such as `offprint.output.exists` fail validation and return
`2`. Use the structured error code to choose recovery within a status family.

Batch, crawl, and doctor can write a result before returning status `1`. Their
records preserve partial outcomes or recovery details.

## Consume capture events

Create a request with `Capture::into_request()` in Rust or
`captures.request(url)` in Node.js and Python, then pass it to
`captures.start(request)` when automation needs progress or cancellation.
[Language examples](../examples/index.md#save-a-page) show complete
event-consumption examples. Every subscription is independent. Resource
progress can be coalesced, so treat its counts as the latest snapshot rather
than a complete event log.

Call `job.cancel()` to request cancellation, then await `job.result()` to
observe terminal cleanup. A cancelled result uses the structured error code
`offprint.runtime.cancelled`.

## Keep naming boundaries explicit

- CLI flags and TOML keys use kebab-case and snake_case where natural.
- Node.js host options use camelCase.
- Python method arguments use snake_case.
- Canonical request, event, result, manifest, and error dictionaries use
  camelCase in every host.

For example, Python calls `verify_format(...)`, while an `ExportRequest`
dictionary still uses `outputDirectory`, `baseName`, and `frontMatter`.

Use the [record reference](../reference/records.md) and generated
[`schemas/`](https://github.com/peter-gy/offprint/tree/main/schemas) for the exact field names.
