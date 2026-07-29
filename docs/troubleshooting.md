# Troubleshoot PageKnot

Start with `doctor`. It checks configuration, browser discovery, collector
compatibility, the managed cache, network policy, and output capabilities.

```console
pageknot doctor
pageknot doctor --json > doctor.json
```

The JSON form preserves stable reason codes and configuration provenance for
automation or issue reports.

## No compatible browser is available

The error code is `pageknot.browser.unavailable`.

Inspect browser candidates, then install the trusted managed revision:

```console
pageknot browser list
pageknot browser install
pageknot doctor
```

On another platform, select a compatible Chrome or Chromium executable:

```console
pageknot doctor --browser-path /path/to/chrome
pageknot capture https://example.com \
  --browser-path /path/to/chrome \
  --output example.html
```

Use `browser.channel = "managed"` in a profile when captures must use the
catalog-pinned browser.

## Capture reaches its deadline

`pageknot.readiness.timeout` means the page did not satisfy the selected
readiness condition. `pageknot.runtime.timeout` means the complete operation
exceeded its deadline.

First give the page a bounded, observable deadline:

```console
pageknot capture https://example.com \
  --wait-until render-idle \
  --timeout 2m \
  --output example.html
```

For a page with continuously open fetch or XMLHttpRequest calls, keep
`render-idle`. For a page with finite requests and delayed worker rendering,
use `network-idle` with a short post-condition delay:

```console
pageknot capture https://example.com \
  --wait-until network-idle \
  --delay 1s \
  --timeout 2m \
  --output example.html
```

Use `load` or `dom-content-loaded` when that browser milestone is the page's
known rendering boundary. These modes can capture before later application
updates.

## A selector fails

`pageknot.selector.invalid` means the selector is invalid CSS.
`pageknot.selector.not_found` means `document.querySelector` found no match in
the top-level document.

Check the first match in the live page:

```js
document.querySelector("main article")
```

Then run:

```console
pageknot capture https://example.com \
  --selector "main article" \
  --output article.html
```

Selector capture targets the top-level document. Content inside a frame needs a
page-level capture.

## A resource is missing

With the default `warn` policy, the capture result records unresolved resources
and can still commit a verified artifact. Require complete resource acquisition
when fidelity depends on every resource:

```console
pageknot capture https://example.com \
  --missing-resources fail \
  --json \
  --output example.html > capture.json
```

Inspect `.resources`, `.warnings`, and the stable error code in the JSON result.
Retryable `pageknot.resource.*` failures can reflect a transient server or
stream error. Budget errors require a larger relevant limit or a smaller
capture.

## Offline verification fails

`pageknot.verification.network` means the staged artifact attempted an external
request during a network-denied reopen. Other `pageknot.verification.*` codes
name the failed manifest, resource, policy, or browser-state check.

Capture a sanitized diagnostic bundle:

```console
pageknot verify example.html \
  --level offline \
  --diagnostics diagnostics
```

Keep the original artifact for inspection. A failed capture or verification
does not replace the requested destination.

## Credential input is rejected

`pageknot.input.credentials` covers invalid JSON, oversized input, shared
stream mode, symbolic links, and unsafe file permissions.

On Unix, restrict the file to its owner:

```console
chmod 600 headers.json
pageknot capture https://example.com/account \
  --headers headers.json \
  --output account.html
```

On Windows, PageKnot checks the file access control list. Put the file in a
private user directory and remove access granted to unrelated principals.

Header and cookie files are limited to 1 MiB. One of them can use stdin. Raw
artifact output cannot share stream mode with credential input.

## Too many files are open

Operating-system error 24 means the process reached its file descriptor limit.
Stop submitting new jobs, close the shared `PageKnot` service, and wait for
owned browser processes to exit before retrying.

For a long-running service:

- Reuse one `PageKnot` instance.
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
descriptors. Run `pageknot doctor` after the service has closed to confirm that
browser acquisition still succeeds.

## An output cannot be committed

PageKnot stages beside the destination and atomically replaces regular files
after verification. Parent-directory, symbolic-link, permission, filesystem,
and synchronization failures use `pageknot.output.*` codes.

Check the directory without removing the existing artifact:

```console
test -d artifacts
test -w artifacts
pageknot doctor
```

Choose a writable local directory on a filesystem that supports atomic create
and replacement. `doctor` reports both capabilities for its current output
directory.

## Prepare an issue report

Include:

- `pageknot --version`
- Operating system and architecture
- `pageknot doctor --json` with private paths reviewed
- Stable error code and pipeline stage
- The smallest URL or local fixture that reproduces the failure
- Sanitized diagnostics path when the command produced one

Treat captured pages, artifacts, credential files, and diagnostics as
potentially sensitive. Review them before sharing.
