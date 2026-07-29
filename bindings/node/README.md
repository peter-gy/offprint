# `@pageknot/node`

`@pageknot/node` captures rendered pages through the PageKnot Rust engine and
returns the canonical capture records as JavaScript objects.

The current package is an alpha build from the PageKnot source checkout. It
requires Node.js 22 or newer, Bun 1.3.14, the repository Rust toolchain, and a
supported native build environment.

## Build and capture

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
import { PageKnot } from "@pageknot/node";

const pageknot = new PageKnot();

try {
  const result = await pageknot.capture("https://example.com", {
    output: "example.html",
  });
  if (result.artifact.kind !== "file") {
    throw new Error("expected a file artifact");
  }
  console.log(result.artifact.path);
} finally {
  await pageknot.close();
}
```

The first capture discovers a compatible local browser. When discovery and the
managed cache are empty, PageKnot downloads and verifies its pinned Chrome for
Testing build.

## Capture options

`capture(url, options)` accepts output, profile, timeout, readiness, viewport,
strict resource handling, browser visibility, conflict policy, scope, CSS
selector, and optimization options. The generated TypeScript declarations in
[`index.d.ts`](./index.d.ts) define the current names and shapes.

Wait for finite requests, then allow one second for worker rendering:

```ts
const result = await pageknot.capture("https://example.com", {
  output: "application.html",
  waitUntil: "network-idle",
  delayMs: 1000,
  timeoutMs: 120_000,
});
```

## Jobs and lifecycle

Call `pageknot.captures.start(request)` when the caller needs progress events or
cancellation. Each call to `job.events()` creates an independent asynchronous
event subscription. `job.result()` resolves once with the terminal capture
record.

One `PageKnot` instance can serve repeated captures. Always await
`pageknot.close()` during shutdown so PageKnot can cancel active jobs and
release its owned browser processes.

## Errors and records

Native failures reject with `PageKnotError`. Inspect `code`, `stage`,
`retryable`, `details`, and `diagnosticsPath` to choose a retry or recovery
action.

Capture results and requests follow the versioned contracts in
[`schemas/`](../../schemas). See the [feature matrix](../../docs/feature-matrix.md)
for interface parity and [troubleshooting](../../docs/troubleshooting.md) for
browser, readiness, verification, and resource recovery.
