# `pageknot`

The `pageknot` package captures a rendered page through the PageKnot Rust engine
and returns the canonical capture records as Python dictionaries.

```python
import asyncio

from pageknot import PageKnot


async def main() -> None:
    async with PageKnot() as pageknot:
        result = await pageknot.capture(
            "https://example.com",
            output="example.html",
        )
        print(result["artifact"])


asyncio.run(main())
```

Set `wait_until="network-idle"` and `delay_ms=1000` to wait for finite
requests, then add a one-second post-condition delay. The delay covers worker
computation that updates the document after request activity ends.

The first capture discovers a compatible local browser and provisions the
pinned managed browser when the cache and system discovery are empty.
Prebuilt wheels cover Linux x86-64 with glibc, macOS x86-64 and arm64, and
Windows x86-64.

Call `pageknot.captures.start(request)` when the caller needs progress events or
cancellation. Each call to `job.events()` creates an independent asynchronous
event subscription.

Failures derive from `PageKnotError`. Stage-specific subclasses let callers
handle validation, browser, navigation, resource, verification, and shutdown
failures independently.
