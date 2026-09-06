import asyncio

from offprint import Offprint


async def main() -> None:
    async with Offprint() as offprint:
        result = await offprint.capture(
            "https://example.com",
            output="example.html",
        )
        print(result["artifact"]["path"])


asyncio.run(main())
