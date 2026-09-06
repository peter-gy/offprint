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
