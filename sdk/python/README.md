# `offprint` for Python

The `offprint` package captures rendered pages through the native Offprint
service and returns canonical capture records as dictionaries.

It supports Python 3.10 through 3.14 on Linux x86-64 with glibc, macOS arm64
and x86-64, and Windows x86-64.

Linux hosts also need Chromium's
[shared runtime libraries](https://github.com/peter-gy/offprint/blob/main/docs/start/install.md#linux-runtime-libraries).

## Install and capture

Install version 0.0.1:

```console
python -m pip install offprint==0.0.1
```

```python
import asyncio

from offprint import Offprint


async def main() -> None:
    async with Offprint() as offprint:
        receipt = await offprint.capture(
            "https://example.com",
            output="example.html",
        )
        artifact = receipt["artifact"]
        assert artifact["kind"] == "file"
        print(artifact["path"])


asyncio.run(main())
```

The first capture discovers a compatible Chrome, Chromium, or Microsoft Edge
installation. When discovery and the managed cache are empty, Offprint
downloads and verifies its pinned
[Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)
build.

## Requests and jobs

Inside the service context, create a request with native defaults, choose
memory output, and start a job:

```python
request = offprint.captures.request("https://example.com")
request["output"] = {"kind": "memory", "maxBytes": 16 * 1024 * 1024}
job = await offprint.captures.start(request)
receipt = await job.result()
```

Use `job.events()` for progress and `job.cancel()` to request cancellation.
Use `async with` for a bounded lifetime, or await `close()` during shutdown.
`offprint.artifacts` inspects, verifies, and exports saved captures.
`offprint.browsers` manages browser discovery and installations.

Methods and keyword arguments use snake_case. Request and result dictionaries
use camelCase. Failures raise `OffprintError` subclasses with a stable `code`,
`stage`, and `retryable` flag.

Read the complete [Python integration guide](https://github.com/peter-gy/offprint/blob/main/docs/integrations/python.md),
[record reference](https://github.com/peter-gy/offprint/blob/main/docs/reference/records.md),
and [troubleshooting guide](https://github.com/peter-gy/offprint/blob/main/docs/operations/troubleshooting.md).

[`src/offprint/__init__.pyi`](./src/offprint/__init__.pyi) owns exact host
signatures. [`src/offprint/contracts.py`](./src/offprint/contracts.py)
owns importable generated canonical dictionary shapes.

Source contributors can follow the
[contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md).
