# `pageknot`

The `pageknot` Python package captures rendered pages through the PageKnot Rust
engine and returns the canonical capture records as dictionaries.

The current package is an alpha build from the PageKnot source checkout. It
requires Python 3.10 or newer, uv, the repository Rust toolchain, and a
supported native build environment.

## Build and capture

From the repository root:

```console
cd bindings/python
uv sync --frozen
uv run maturin develop
uv run python examples/capture.py
```

The example writes and prints:

```text
example.html
```

Its complete source is [`examples/capture.py`](./examples/capture.py):

```python
import asyncio

from pageknot import PageKnot


async def main() -> None:
    async with PageKnot() as pageknot:
        result = await pageknot.capture(
            "https://example.com",
            output="example.html",
        )
        print(result["artifact"]["path"])


asyncio.run(main())
```

The first capture discovers a compatible local browser. When discovery and the
managed cache are empty, PageKnot downloads and verifies its pinned Chrome for
Testing build.

## Capture options

`capture(url, **options)` accepts output, profile, timeout, readiness, viewport,
strict resource handling, browser visibility, conflict policy, scope, CSS
selector, and optimization options. The public method signatures in
[`python/pageknot/_api.py`](./python/pageknot/_api.py) define the current names.

Wait for finite requests, then allow one second for worker rendering:

```python
result = await pageknot.capture(
    "https://example.com",
    output="application.html",
    wait_until="network-idle",
    delay_ms=1000,
    timeout_ms=120_000,
)
```

## Jobs and lifecycle

Call `pageknot.captures.start(request)` when the caller needs progress events or
cancellation. Each call to `job.events()` creates an independent asynchronous
event subscription. `await job.result()` returns the terminal capture record.

Use `async with PageKnot()` for one bounded service lifetime. Long-running
callers can reuse one instance and await `pageknot.close()` during shutdown.

## Errors and records

Failures derive from `PageKnotError`. Stage-specific subclasses let callers
handle validation, browser, navigation, resource, verification, and shutdown
failures independently. Each error carries a stable code, stage, retryability,
details, and an optional diagnostics path.

Capture results and requests follow the versioned contracts in
[`schemas/`](../../schemas). See the [feature matrix](../../docs/feature-matrix.md)
for interface parity and [troubleshooting](../../docs/troubleshooting.md) for
browser, readiness, verification, and resource recovery.
