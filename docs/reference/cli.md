# CLI reference

The `offprint` command writes command data to standard output and diagnostics
to standard error. Run `offprint <command> --help` for the exact parser-owned
flag inventory in the installed version.

## Command tree

| Command | Input | Result |
| --- | --- | --- |
| `capture <URL>` | HTTP, HTTPS, or allowed `file:` URL plus required output | Verified Offprint HTML file or raw bytes |
| `artifact inspect <ARTIFACT>` | HTML path or standard input | Embedded `ArtifactManifest` |
| `artifact verify <ARTIFACT>` | HTML path or standard input | Static or offline `ArtifactVerification` |
| `artifact verify <PATH> --format <FORMAT>` | Exported artifact path | Format-specific `ArtifactVerification` |
| `artifact export <ARTIFACT>` | HTML path or standard input, output directory, formats | Verified `ExportResult` |
| `batch <BATCH_REQUEST>` | Versioned JSON file or standard input | `BatchResult` with one outcome per descriptor |
| `crawl <URL>` | Seed URL, output directory, bounds | `CrawlResult` in breadth-first order |
| `doctor` | Resolved configuration and optional browser override | `BrowserDoctorReport` and recovery actions |
| `browser install` | Optional catalog revision | Installed managed-browser record |
| `browser list` | Optional cache directory | Browser candidate inventory |
| `browser remove <REVISION>` | Installed managed revision | Removal record |
| `completion <SHELL>` | Bash, Elvish, Fish, PowerShell, or Zsh | Completion script |

## `capture`

```text
offprint capture <URL> --output <PATH|-> [OPTIONS]
```

Major option groups:

| Group | Options |
| --- | --- |
| Output | `--output`, `--on-exists` |
| Configuration | `--config`, `--profile` |
| Browser | `--browser-path`, `--cdp-url`, `--headed` |
| Environment | `--viewport`, `--locale`, `--timezone`, `--color-scheme` |
| Readiness | `--timeout`, `--wait-until`, `--delay` |
| Content | `--missing-resources`, `--scope`, `--selector`, optimization flags |
| Trust | `--headers`, `--cookies`, `--network-policy`, `--verification` |
| Output protocol | `--json`, `--quiet`, `--color`, `--diagnostics` |

`--browser-path` conflicts with `--cdp-url`. `--selector` conflicts with any
explicit `--scope` value. `--json` conflicts with raw artifact output. Standard
input can serve one artifact, batch request, header file, or cookie file at a
time.

## `artifact export`

```text
offprint artifact export <ARTIFACT|-> \
  --output <DIRECTORY> \
  --format <FORMAT>...
```

Repeat `--format` or pass a comma-separated list. Accepted values are `pdf`,
`markdown`, `zip`, `self-extracting-html`, and `mhtml`.

`--landscape` and `--prefer-css-page-size` require PDF. `--no-front-matter`
requires Markdown. `--base-name` is normalized into portable output names.
Conflict policy applies to the output set and defaults to `fail`.

## `batch`

```text
offprint batch <BATCH_REQUEST|-> \
  [--config <PATH>] \
  [--browser-path <PATH> | --cdp-url <URL>] \
  [--json] [--quiet] [--color <MODE>]
```

The JSON document owns complete capture requests and concurrency. Browser flags
replace `auto` selection and preserve explicit request selections. A partial
failure writes `BatchResult`, then returns status `1`.

## `crawl`

```text
offprint crawl <URL> --output <DIRECTORY> \
  [--max-pages <COUNT>] [--max-depth <COUNT>] \
  [--concurrency <COUNT>] [--resume <PATH>] \
  [--retry-failed] [--allow-cross-origin]
```

Defaults are 100 pages, depth 3, concurrency 4, and the seed-origin boundary.
`--retry-failed` requires `--resume`. Crawl uses offline verification and a
local browser. Each verified page replaces an existing generated path in the
output directory.

## `artifact verify`

Without `--format`, the command verifies Offprint HTML and defaults to offline
mode. With `--format`, it runs the corresponding export verifier and requires a
filesystem path. HTML verification options do not apply to export formats.

## `doctor` and browser commands

After configuration and service construction succeed, `doctor` returns a report
in JSON mode. When `ready` is false, it writes that report before returning
status `1`. It may launch a local probe or connect to a remote endpoint.

Browser commands never prompt. `remove --force` still refuses an active lease
and requires another compatible browser when removing the selected revision.

## Standard output and standard error

| Mode | Standard output | Standard error |
| --- | --- | --- |
| Human success | Paths, summaries, reports, or completion script | Progress and warnings |
| `--quiet` | Same command data | Errors |
| `--json` | One versioned JSON value | One structured error on failure |
| `--output -` | Verified artifact bytes | Diagnostics |

Capture JSON suppresses human progress. Batch, crawl, and doctor may return a
structured value before a failure status.

## Exit statuses

| Status | Contract |
| ---: | --- |
| `0` | Requested operation completed |
| `1` | Runtime, browser, scheduler, artifact input, or output operation failed |
| `2` | Arguments or configuration were invalid |
| `3` | Artifact verification rejected the input |
| `130` | Operation was interrupted |

Artifact readability and size failures return `1`, even when discovered during
a verification command. See [errors](./errors.md) for structured recovery.
