import asyncio

from offprint import Offprint


async def capture_memory() -> None:
    async with Offprint() as offprint:
        request = offprint.captures.request("https://example.com")
        request["output"] = {"kind": "memory", "maxBytes": 16 * 1024 * 1024}
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
