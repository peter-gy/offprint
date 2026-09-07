import asyncio

from offprint import Offprint


async def main() -> None:
    async with Offprint() as offprint:
        result = await offprint.capture(
            "https://example.com",
            output="example.html",
        )
        artifact = result["artifact"]
        assert artifact["kind"] == "file"
        print(artifact["path"])


asyncio.run(main())
