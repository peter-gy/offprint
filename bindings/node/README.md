# `@pageknot/node`

`@pageknot/node` captures a rendered page through the PageKnot Rust engine and
returns the canonical capture records as JavaScript objects.

```ts
import { PageKnot } from "@pageknot/node";

const pageknot = new PageKnot();

try {
  const result = await pageknot.capture("https://example.com", {
    output: "example.html",
  });
  console.log(result.artifact);
} finally {
  await pageknot.close();
}
```

Set `waitUntil: "network-idle"` and `delayMs: 1000` to wait for finite
requests, then add a one-second post-condition delay. The delay covers worker
computation that updates the document after request activity ends.

The first capture discovers a compatible local browser and provisions the
pinned managed browser when the cache and system discovery are empty.
Prebuilt packages cover Linux x86-64 with glibc, macOS x86-64 and arm64, and
Windows x86-64.

Call `pageknot.captures.start(request)` when the caller needs progress events or
cancellation. Each call to `job.events()` creates an independent asynchronous
event subscription.

Native failures reject with `PageKnotError`. Inspect `code`, `stage`,
`retryable`, `details`, and `diagnosticsPath` to decide whether to retry or
surface a recovery action.
