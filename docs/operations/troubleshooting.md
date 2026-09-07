# Troubleshoot Offprint

Start with the operational report:

```console
offprint doctor
offprint doctor --json > doctor.json
```

The JSON report contains stable reason codes, selected browser information,
cache state, collector compatibility, output capability, selected configuration
values, network summary, and recovery actions.

## No compatible browser is available

The usual code is `offprint.browser.unavailable`.

```console
offprint browser list
offprint browser install
offprint doctor
```

Select a compatible executable when installation belongs to the host:

```console
offprint doctor --browser-path /path/to/chrome
```

System discovery accepts Chrome, Chromium, and Microsoft Edge based on
Chromium 120 or newer.

## Capture reaches its deadline

`offprint.readiness.timeout` means the page did not satisfy readiness.
`offprint.runtime.timeout` means the complete operation exceeded its deadline.

Start with the broad default and an explicit total timeout:

```console
offprint capture https://example.com \
  --wait-until render-idle \
  --timeout 2m \
  --output example.html
```

For finite requests followed by worker rendering, try network idle with a short
delay. Use load milestones only when they define the page's true rendering
boundary. See [control capture](../guides/control-capture.md).

## A selector or selection fails

- `offprint.selector.invalid`: invalid CSS selector syntax
- `offprint.selector.not_found`: no top-level match
- `offprint.selection.empty`: no active non-collapsed top-level selection

Check the first selector match in the live page:

```js
document.querySelector("main article");
```

Selector and active-selection capture do not search inside child frames.

## A resource is missing

The default `warn` policy can commit with a failed resource record and inert
fallback. Inspect the receipt summary and artifact manifest:

The example uses [jq](https://jqlang.org/), a command-line JSON query tool.

```console
offprint artifact inspect example.html --json > manifest.json
jq '.resourceRecords[] | select(.outcome.kind == "failed")' manifest.json
```

Retryable resource failures can reflect a transient server, stream, or browser
response problem. Limit errors require a larger relevant limit or smaller
capture. Authentication failures may require a correctly scoped cookie or
same-origin header.

## Verification fails

`offprint.verification.network` means offline reopen observed a request. Other
`offprint.verification.*` codes name manifest, policy, owned-script, resource,
frame, structure, or browser-state checks.

Capture a sanitized diagnostic bundle:

```console
offprint artifact verify example.html \
  --verification offline \
  --diagnostics diagnostics
```

Keep the original artifact. Verification failure does not replace the requested
destination.

## Credential input is rejected

`offprint.input.credentials` covers invalid JSON, entry limits, invalid names
or scopes, shared stream mode, symbolic links, and unsafe permissions.

```console
chmod 600 headers.json
offprint capture https://example.com/account \
  --headers headers.json \
  --output account.html
```

On Windows, store credentials in a private user directory and remove access for
unrelated principals.

## Too many files are open

Operating-system error 24 means the process exhausted its file descriptor
budget.

- Reuse one `Offprint` service.
- Bound capture, batch, and crawl concurrency.
- Lower `browser.maximum_contexts` and `concurrent_resources`.
- Await terminal results and service shutdown.

Inspect the Unix shell limit with `ulimit -n`. Lower concurrency before raising
the host limit.

## Output cannot be committed

`offprint.output.*` codes cover conflicts, parent directories, symbolic links,
permissions, unsupported filesystem guarantees, staging identity, commit, and
recovery.

Check the intended directory without deleting existing output:

```console
test -d artifacts
test -w artifacts
offprint doctor
```

For exports, preserve `transactionPath` and `recoveryPaths` from error details.
Retrying the same output directory can complete journal recovery.

## Prepare an issue report

Include:

- `offprint --version`
- Operating system and architecture
- Reviewed `offprint doctor --json` output
- Error code, stage, and structured details
- Smallest URL or local fixture
- Diagnostic path when available

Treat URLs, artifacts, credentials, diagnostics, and screenshots as potentially
sensitive.
