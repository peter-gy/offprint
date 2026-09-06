# `offprint` for Node.js

The `offprint` package captures rendered pages through the native Offprint
service and returns canonical capture records as JavaScript objects.

It requires Node.js 22 or newer and supports Linux x86-64 with glibc, macOS
arm64 and x86-64, and Windows x86-64.

Linux hosts also need Chromium's
[shared runtime libraries](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md#linux-runtime-libraries).

## Install and capture

After release 0.1.0 is published, install its explicit version:

```console
npm install offprint@0.1.0
```

Use the [source-checkout steps](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md)
when working before publication.

```js
import { Offprint } from "offprint";

const offprint = new Offprint();
try {
  const receipt = await offprint.capture("https://example.com", {
    output: "example.html",
  });
  console.log(receipt.artifact.path);
} finally {
  await offprint.close();
}
```

The first capture discovers a compatible Chrome, Chromium, or Microsoft Edge
installation. When discovery and the managed cache are empty, Offprint
downloads and verifies its pinned
[Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)
build.

## Services and lifecycle

`Offprint` exposes:

- `captures` for jobs, events, cancellation, batches, and crawls
- `artifacts` for manifest inspection, HTML verification, export, and
  format-specific verification
- `browsers` for discovery, managed installation, inventory, removal,
  diagnosis, and idle close

The shorthand `capture` method writes one file. Pass a complete
`CaptureRequest` to `captures.start` for memory output, credentials, a custom
network policy, complete environment control, diagnostics, or exact limits.

Every `job.events()` call creates an independent asynchronous iterator.
`job.cancel()` is idempotent. `job.result()` resolves once with a
`CaptureReceipt` or rejects with `OffprintError`.

Always await `close()` or use `Symbol.asyncDispose` during orderly shutdown.
Finalization and process-exit handling are fallback safeguards.

## Records and errors

Host options and methods use camelCase. Canonical request, event, result,
manifest, browser, batch, crawl, and error records also use camelCase.

`OffprintError` exposes `code`, `stage`, `retryable`, `details`,
`diagnosticsPath`, and an optional nested `source`.

Read the complete [Node.js integration guide](https://github.com/peter-gy/offprint/blob/main/docs/integrations/node.md),
[record reference](https://github.com/peter-gy/offprint/blob/main/docs/reference/records.md),
and [troubleshooting guide](https://github.com/peter-gy/offprint/blob/main/docs/operations/troubleshooting.md).

[`index.d.ts`](./index.d.ts) owns exact TypeScript signatures and exported
record aliases.

Source contributors can follow the
[contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md).
