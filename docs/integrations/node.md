# Use Offprint from Node.js

The `offprint` npm package exposes Promise-based services and the canonical
camelCase record types. It requires Node.js 22 or newer.

Build the current checkout with the steps in [Install Offprint](../start/install.md).
Tagged releases install from npm as `offprint@VERSION`.

## Capture one file

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

The shorthand `capture` method requires a file output. It accepts common
profile, timeout, readiness, viewport, fidelity, headed, conflict, network,
verification, selection, and optimization options.

## Start a complete request

Create a request with `captures.request`, change the policies your task needs,
then call `captures.start` for progress events and cancellation:

```js
import { Offprint } from "offprint";

const offprint = new Offprint();
try {
  const request = offprint.captures.request("https://example.com");
  request.output = { kind: "memory", maxBytes: 16 * 1024 * 1024 };
  const job = await offprint.captures.start(request);
  for await (const event of job.events()) {
    if (event.type === "warning") {
      console.error(event.warning.code);
    }
  }

  const receipt = await job.result();
  if (receipt.artifact.kind !== "bytes") {
    throw new Error("expected a memory artifact");
  }
  const html = Uint8Array.from(receipt.artifact.content);
  console.log(html.byteLength, receipt.artifact.sha256);
} finally {
  await offprint.close();
}
```

`captures.request(url, options?)` returns a fresh canonical request using the
same profiles and options as `capture`. Its default output is bounded memory.
Set credentials, network rules, browser environment, diagnostics, or exact
limits directly on the request. `start` validates the completed request before
acquiring a browser.

Every `events()` call creates an independent subscription. `cancel()` is
idempotent. Await `result()` to observe terminal cleanup.

Canonical JSON represents in-memory content as `number[]`. Convert it to a
`Uint8Array` before passing the artifact to byte-oriented Node.js APIs.
The typed version lives in
[`capture-memory.ts`](../../bindings/node/examples/capture-memory.ts) and is
checked by `just node-check`.

## Public services

| Property | Methods |
| --- | --- |
| `captures` | `request`, `start`, `batch`, `crawl` |
| `artifacts` | `inspect`, `verify`, `export`, `verifyFormat` |
| `browsers` | `ensure`, `install`, `list`, `remove`, `doctor`, `closeIdle` |

The [service API reference](../reference/service-api.md) defines constructor
options, defaults, each method's return and failure boundary, and lifecycle.

Native failures reject with `OffprintError`. Inspect `code`, `stage`,
`retryable`, `details`, `diagnosticsPath`, and `source`.

## Runtime and platform support

The package contains addons for:

- Linux x86-64 with glibc
- macOS arm64
- macOS x86-64
- Windows x86-64

The loader rejects an unsupported platform. When a supported target's addon is
missing or fails to load, the error reports the expected filename. The package
exposes ECMAScript module named exports and a CommonJS `require("offprint")`
entrypoint.

Call and await `close()` or use `Symbol.asyncDispose` during orderly shutdown.
The finalizer and process-exit handling are fallback safeguards. Child
services, jobs, and event iterators retain the root runtime until their work
completes.

[`index.d.ts`](../../bindings/node/index.d.ts) is the exact Node API and record
reference. Canonical record fields remain camelCase.
