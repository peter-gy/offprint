from __future__ import annotations

import asyncio
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from pageknot import PageKnot


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/asset.svg":
            content_type = "image/svg+xml"
            body = (
                b'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">'
                b'<rect width="8" height="8" fill="rgb(24,96,160)"/></svg>'
            )
        else:
            content_type = "text/html; charset=utf-8"
            body = (
                b"<!doctype html><title>Installed wheel</title>"
                b'<h1 id="state">waiting</h1><img src="/asset.svg">'
                b'<script>document.querySelector("#state").textContent='
                b'"installed wheel rendered"</script>'
            )
        self.send_response(200)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


async def run() -> None:
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="pageknot-wheel-") as directory:
            output = Path(directory) / "capture.html"
            browser_path = os.environ.get("PAGEKNOT_PACKAGE_BROWSER_PATH")
            options = {"browser_path": browser_path} if browser_path else None
            async with PageKnot(options) as pageknot:
                assert not hasattr(pageknot, "_test_panic")
                result = await pageknot.capture(
                    f"http://127.0.0.1:{server.server_port}/",
                    output=output,
                )
            assert result["status"] == "succeeded"
            assert result["verification"]["passed"] is True
            assert result["verification"]["networkRequests"] == 0
            artifact = output.read_text(encoding="utf-8")
            assert "installed wheel rendered" in artifact
            assert "data:image/svg+xml;base64," in artifact
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


asyncio.run(run())
