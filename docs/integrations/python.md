# Use Offprint from Python

The `offprint` Python package exposes `asyncio` services over the native Rust
runtime. It supports Python 3.10 through 3.14.

Build the current checkout with the steps in [Install Offprint](../start/install.md).
Tagged releases install from PyPI as `offprint==VERSION`.

## Capture one file

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

The shorthand `capture` method requires a file output. Its keyword arguments
use snake_case, including `wait_until`, `timeout_ms`, and
`remove_unused_css`.

## Canonical dictionaries use camelCase

Complete requests and every returned record preserve the canonical camelCase
schema across Rust JSON, the CLI, Node.js, and Python.

Inside the `async with Offprint() as offprint` block from the first example:

```python
exported = await offprint.artifacts.export(
    "example.html",
    {
        "schemaVersion": 2,
        "outputDirectory": "exports",
        "baseName": "example",
        "conflict": "fail",
        "formats": [
            {"format": "markdown", "options": {"frontMatter": True}},
            {"format": "zip"},
        ],
    },
)
```

Canonical record types are importable from the package root and from generated
[`contracts.py`](../../bindings/python/python/offprint/contracts.py):

```python
from offprint import CaptureRequest, CaptureReceipt, ExportRequest
```

The generated module is the exact dictionary shape reference.

## Jobs and services

This complete request uses memory output and disables lazy-load scrolling to
keep the first job example short and deterministic:

```python
import asyncio

from offprint import CaptureRequest, Offprint


request: CaptureRequest = {
    "schemaVersion": 2,
    "url": "https://example.com/",
    "output": {"kind": "memory", "maxBytes": 16 * 1024 * 1024},
    "browser": {"kind": "auto"},
    "environment": {
        "viewport": {"width": 1440, "height": 900, "scale": 1},
        "locale": "en-US",
        "timezone": "UTC",
        "colorScheme": "light",
        "reducedMotion": "reduce",
        "userAgent": {"kind": "browser-default"},
    },
    "readiness": {
        "mode": "render-idle",
        "networkQuiet": 500,
        "mutationQuiet": 300,
        "delay": 0,
        "lazyLoad": {"kind": "disabled"},
    },
    "content": {
        "missingResources": "warn",
        "preservePasswordValues": False,
    },
    "network": {"kind": "standard"},
    "limits": {
        "duration": 120000,
        "redirects": 20,
        "frames": 256,
        "nodes": 1000000,
        "resources": 10000,
        "resourceBytes": 64 * 1024 * 1024,
        "totalResourceBytes": 512 * 1024 * 1024,
        "collectorChunkBytes": 1024 * 1024,
        "concurrentResources": 8,
        "artifactBytes": 64 * 1024 * 1024,
        "resourceRecursionDepth": 64,
        "frameDepth": 64,
    },
    "verification": "offline",
    "diagnostics": {},
}


async def capture_memory() -> None:
    async with Offprint() as offprint:
        job = await offprint.captures.start(request)
        async for event in job.events():
            if event["type"] == "warning":
                print(event["warning"]["code"])

        receipt = await job.result()
        artifact = receipt["artifact"]
        if artifact["kind"] != "bytes":
            raise RuntimeError("expected a memory artifact")
        html = bytes(artifact["content"])
        print(len(html), artifact["sha256"])


asyncio.run(capture_memory())
```

Canonical dictionaries represent in-memory content as `list[int]`. Convert it
to `bytes` before passing the artifact to byte-oriented Python APIs.
The same example lives in
[`capture_memory.py`](../../bindings/python/examples/capture_memory.py) and is
type-checked by `just python-check`.

| Property | Methods |
| --- | --- |
| `captures` | `start`, `batch`, `crawl` |
| `artifacts` | `inspect`, `verify`, `export`, `verify_format` |
| `browsers` | `ensure`, `install`, `list`, `remove`, `doctor`, `close_idle` |

The [service API reference](../reference/service-api.md) defines constructor
options, defaults, each method's return and failure boundary, and lifecycle.

Python maps error stages to subclasses of `OffprintError`, including
`ValidationError`, `BrowserError`, `ReadinessError`, `ResourceError`,
`VerificationError`, and `ShutdownError`. Every exception also exposes the
canonical code, stage, retryability, details, optional diagnostics path, and
nested source.

Use `async with` for a bounded lifetime. Long-running services can reuse one
`Offprint` instance and await `close()` during shutdown. Weak-reference and
process-exit finalizers are fallback safeguards.

Release wheels use Python's
[stable application binary interface](https://docs.python.org/3/c-api/stable.html)
from Python 3.10 and target Linux x86-64
with glibc, macOS arm64 and x86-64, and Windows x86-64. Building from source
requires the repository Rust toolchain, uv, and Maturin.
