# `offprint` for Python

The `offprint` package captures rendered pages through the native Offprint
service and returns canonical capture records as dictionaries.

It supports Python 3.10 through 3.14 on Linux x86-64 with glibc, macOS arm64
and x86-64, and Windows x86-64.

Linux hosts also need Chromium's
[shared runtime libraries](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md#linux-runtime-libraries).

## Install and capture

After release 0.1.0 is published, install its explicit version:

```console
python -m pip install offprint==0.1.0
```

Use the [source-checkout steps](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md)
when working before publication.

```python
import asyncio

from offprint import Offprint


async def main() -> None:
    async with Offprint() as offprint:
        receipt = await offprint.capture(
            "https://example.com",
            output="example.html",
        )
        print(receipt["artifact"]["path"])


asyncio.run(main())
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
`CaptureRequest` dictionary to `captures.start` for memory output, credentials,
a custom network policy, complete environment control, diagnostics, or exact
limits.

Use `async with` for a bounded service lifetime. A long-running service can
reuse one instance and await `close()` during shutdown.

## Naming and errors

Python methods and keyword arguments use snake_case. Canonical request, event,
result, manifest, browser, batch, crawl, and error dictionaries remain
camelCase.

Python maps failure stages to subclasses of `OffprintError`. Every exception
also exposes the stable code, stage, retryability, details, optional diagnostics
path, and nested source.

Read the complete [Python integration guide](https://github.com/peter-gy/offprint/blob/main/docs/integrations/python.md),
[record reference](https://github.com/peter-gy/offprint/blob/main/docs/reference/records.md),
and [troubleshooting guide](https://github.com/peter-gy/offprint/blob/main/docs/operations/troubleshooting.md).

[`python/offprint/__init__.pyi`](./python/offprint/__init__.pyi) owns exact host
signatures. [`python/offprint/contracts.py`](./python/offprint/contracts.py)
owns importable generated canonical dictionary shapes.

Source contributors can follow the
[contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md).
