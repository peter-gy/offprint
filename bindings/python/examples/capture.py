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
