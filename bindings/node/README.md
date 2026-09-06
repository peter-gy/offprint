# `offprint`

`offprint` captures rendered pages through the Offprint Rust engine and
returns the canonical capture records as JavaScript objects.

The package requires Node.js 22 or newer. Building it from source also requires
Bun 1.3.14, the repository Rust toolchain, and a supported native build
environment.

## Install and capture

```console
npm install offprint
```

```ts
import { Offprint } from "offprint";

await using offprint = new Offprint();
const result = await offprint.capture("https://example.com", {
  output: "example.html",
});
console.log(result.artifact);
```

## Build from source

From the repository root:

```console
cd bindings/node
bun install --frozen-lockfile
bun run build
bun run examples/capture.ts
```

The example writes and prints:

```text
example.html
```

Its complete source is [`examples/capture.ts`](./examples/capture.ts):

```ts
import { Offprint } from "offprint";

await using offprint = new Offprint();
const result = await offprint.capture("https://example.com", {
  output: "example.html",
});
if (result.artifact.kind !== "file") {
  throw new Error("expected a file artifact");
}
console.log(result.artifact.path);
```

The first capture discovers a compatible local browser. When discovery and the
managed cache are empty, Offprint downloads and verifies its pinned Chrome for
Testing build.

## Capture options

`capture(url, options)` requires an output path. It accepts profile, timeout,
readiness, viewport, strict resource handling, browser visibility, conflict
policy, network policy, verification mode, scope, CSS selector, and
optimization options. The generated TypeScript declarations in
[`index.d.ts`](./index.d.ts) define the current names and shapes.

Wait for finite requests, then allow one second for worker rendering:

```ts
const result = await offprint.capture("https://example.com", {
  output: "application.html",
  waitUntil: "network-idle",
  delayMs: 1000,
  timeoutMs: 120_000,
});
```

## Jobs and lifecycle

Call `offprint.captures.start(request)` when the caller needs progress events or
cancellation. Each call to `job.events()` creates an independent asynchronous
event subscription. `job.result()` resolves once with a successful
`CaptureReceipt`. Failure and cancellation reject with `OffprintError`.

One `Offprint` instance can serve repeated captures. Always await
`offprint.close()` during shutdown so Offprint can cancel active jobs and
release its owned browser processes.

`offprint.browsers` exposes `ensure`, `list`, `install`, `remove`, `doctor`, and
`closeIdle` for browser setup and administration.

## Errors and records

Native failures reject with `OffprintError`. Inspect `code`, `stage`,
`retryable`, `details`, and `diagnosticsPath` to choose a retry or recovery
action.

Capture results and requests follow the versioned contracts in
[`schemas/`](../../schemas). See the [feature matrix](../../docs/feature-matrix.md)
for interface parity and [troubleshooting](../../docs/troubleshooting.md) for
browser, readiness, verification, and resource recovery.
