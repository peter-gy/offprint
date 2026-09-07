# `offprint` for Node.js

The `offprint` package captures rendered pages through the native Offprint
service and returns canonical capture records as JavaScript objects.

It requires Node.js 22 or newer and supports Linux x86-64 with glibc, macOS
arm64 and x86-64, and Windows x86-64.

Linux hosts also need Chromium's
[shared runtime libraries](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md#linux-runtime-libraries).

## Install and capture

Install version 0.0.1:

```console
npm install offprint@0.0.1
```

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

## Requests and jobs

Inside the `try` block, create a request with native defaults, choose memory
output, and start a job:

```js
const request = offprint.captures.request("https://example.com");
request.output = { kind: "memory", maxBytes: 16 * 1024 * 1024 };
const job = await offprint.captures.start(request);
const receipt = await job.result();
```

Use `job.events()` for progress and `job.cancel()` to request cancellation.
Always await `offprint.close()` or use `Symbol.asyncDispose` during shutdown.
`offprint.artifacts` inspects, verifies, and exports saved captures.
`offprint.browsers` manages browser discovery and installations.

Native failures reject with `OffprintError`, including a stable `code`,
`stage`, and `retryable` flag.

Read the complete [Node.js integration guide](https://github.com/peter-gy/offprint/blob/main/docs/integrations/node.md),
[record reference](https://github.com/peter-gy/offprint/blob/main/docs/reference/records.md),
and [troubleshooting guide](https://github.com/peter-gy/offprint/blob/main/docs/operations/troubleshooting.md).

[`index.d.ts`](./index.d.ts) owns exact TypeScript signatures and exported
record aliases.

Source contributors can follow the
[contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md).
