# Troubleshoot Offprint

Start with `doctor`. It checks configuration, browser discovery, collector
compatibility, the managed cache, network policy, and output capabilities.

```console
offprint doctor
offprint doctor --json > doctor.json
```

The JSON form preserves stable reason codes and configuration provenance for
automation or issue reports.

## No compatible browser is available

The error code is `offprint.browser.unavailable`.

Inspect browser candidates, then install the trusted managed revision:

```console
offprint browser list
offprint browser install
offprint doctor
```

On another platform, select a compatible Chrome or Chromium executable:

```console
offprint doctor --browser-path /path/to/chrome
offprint capture https://example.com \
  --browser-path /path/to/chrome \
  --output example.html
```

Use `browser.channel = "managed"` in a profile when captures must use the
catalog-pinned browser.

## Capture reaches its deadline

`offprint.readiness.timeout` means the page did not satisfy the selected
readiness condition. `offprint.runtime.timeout` means the complete operation
exceeded its deadline.

First give the page a bounded, observable deadline:

```console
offprint capture https://example.com \
  --wait-until render-idle \
  --timeout 2m \
  --output example.html
```

For a page with continuously open fetch or XMLHttpRequest calls, keep
`render-idle`. For a page with finite requests and delayed worker rendering,
use `network-idle` with a short post-condition delay:

```console
offprint capture https://example.com \
  --wait-until network-idle \
  --delay 1s \
  --timeout 2m \
  --output example.html
```

Use `load` or `dom-content-loaded` when that browser milestone is the page's
known rendering boundary. These modes can capture before later application
updates.

## A selector fails

`offprint.selector.invalid` means the selector is invalid CSS.
`offprint.selector.not_found` means `document.querySelector` found no match in
the top-level document.

Check the first match in the live page:

```js
document.querySelector("main article")
```

Then run:

```console
offprint capture https://example.com \
  --selector "main article" \
  --output article.html
```

Selector capture targets the top-level document. Content inside a frame needs a
page-level capture.

## A resource is missing

With the default `warn` policy, the capture receipt records unresolved resources
and can still commit a verified artifact. Require complete resource acquisition
when fidelity depends on every resource:

```console
offprint capture https://example.com \
  --missing-resources fail \
  --json \
  --output example.html > capture.json
```

Inspect `.resources`, `.warnings`, and the stable error code in the JSON result.
Retryable `offprint.resource.*` failures can reflect a transient server or
stream error. Budget errors require a larger relevant limit or a smaller
capture.

## Offline verification fails

`offprint.verification.network` means the staged artifact attempted an external
request during a network-denied reopen. Other `offprint.verification.*` codes
name the failed manifest, resource, policy, or browser-state check.

Capture a sanitized diagnostic bundle:

```console
offprint artifact verify example.html \
  --verification offline \
  --diagnostics diagnostics
```

Keep the original artifact for inspection. A failed capture or verification
does not replace the requested destination.

## Credential input is rejected

`offprint.input.credentials` covers invalid JSON, oversized input, shared
stream mode, symbolic links, and unsafe file permissions.

On Unix, restrict the file to its owner:

```console
chmod 600 headers.json
offprint capture https://example.com/account \
  --headers headers.json \
  --output account.html
```

On Windows, Offprint checks the file access control list. Put the file in a
private user directory and remove access granted to unrelated principals.

Header and cookie files are limited to 1 MiB. One of them can use stdin. Raw
artifact output cannot share stream mode with credential input.

## Too many files are open

Operating-system error 24 means the process reached its file descriptor limit.
Stop submitting new jobs, close the shared `Offprint` service, and wait for
owned browser processes to exit before retrying.

For a long-running service:

- Reuse one `Offprint` instance.
- Await every job's terminal result.
- Call and await `close()` during shutdown.
- Keep `browser.maximum_contexts` and
  `profile.NAME.limits.concurrent_resources` within the host budget.
- Keep batch and crawl concurrency bounded.

On macOS or Linux, inspect the current shell limit:

```console
ulimit -n
```

Lower capture concurrency first. Raise the operating-system limit through the
host's service manager when the measured workload still requires more
descriptors. Run `offprint doctor` after the service has closed to confirm that
browser acquisition still succeeds.

## An output cannot be committed

Offprint stages beside the destination and atomically replaces regular files
after verification. Parent-directory, symbolic-link, permission, filesystem,
and synchronization failures use `offprint.output.*` codes.

Check the directory without removing the existing artifact:

```console
test -d artifacts
test -w artifacts
offprint doctor
```

Choose a writable local directory on a filesystem that supports atomic create
and replacement. `doctor` reports both capabilities for its current output
directory.

## Prepare an issue report

Include:

- `offprint --version`
- Operating system and architecture
- `offprint doctor --json` with private paths reviewed
- Stable error code and pipeline stage
- The smallest URL or local fixture that reproduces the failure
- Sanitized diagnostics path when the command produced one

Treat captured pages, artifacts, credential files, and diagnostics as
potentially sensitive. Review them before sharing.
