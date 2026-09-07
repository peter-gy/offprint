from __future__ import annotations

import asyncio
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

import pytest

from offprint import Offprint, ShutdownError

ASYNC_OPERATION_TIMEOUT = 15.0


class FixtureHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/asset.svg":
            content_type = "image/svg+xml"
            body = (
                b'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">'
                b'<rect width="8" height="8" fill="rgb(24, 96, 160)"/></svg>'
            )
        else:
            content_type = "text/html; charset=utf-8"
            body = (
                b"<!doctype html><html><head><title>Python binding</title></head>"
                b'<body><p>outside selector</p><main id="capture">'
                b'<h1 id="state">waiting</h1><img src="/asset.svg"></main>'
                b'<script>document.querySelector("#state").textContent = '
                b'"binding rendered"</script></body></html>'
            )
        self.send_response(200)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


@pytest.fixture
def source_url() -> Any:
    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@pytest.mark.asyncio
async def test_capture_event_cancellation_and_close_contract(
    source_url: str,
    tmp_path: Path,
) -> None:
    output = tmp_path / "python-binding.html"
    cancelled_output = tmp_path / "python-cancelled.html"
    offprint = Offprint()

    try:
        output.write_text("stale artifact", encoding="utf-8")
        result = await offprint.capture(
            source_url,
            output=output,
            selector="#capture",
            conflict="replace",
        )

        assert result["schemaVersion"] == 2
        assert result["artifact"]["kind"] == "file"
        assert result["artifact"]["path"] == str(output)
        assert result["verification"]["networkRequests"] == 0
        content = output.read_text(encoding="utf-8")
        assert "binding rendered" in content
        assert "data:image/svg+xml;base64," in content
        assert "outside selector" not in content
        assert "stale artifact" not in content

        manifest = await offprint.artifacts.inspect(output)
        assert manifest["schemaVersion"] == 2
        assert manifest["source"]["finalUrl"] == source_url
        assert manifest["resources"]["embedded"] >= 1

        exported = await offprint.artifacts.export(
            output,
            {
                "schemaVersion": 2,
                "outputDirectory": str(tmp_path / "python-formats"),
                "baseName": "python-binding",
                "conflict": "fail",
                "formats": [
                    {"format": "pdf", "options": {}},
                    {
                        "format": "markdown",
                        "options": {"frontMatter": True},
                    },
                    {"format": "zip"},
                    {"format": "self-extracting-html"},
                    {"format": "mhtml"},
                ],
            },
        )
        assert exported["capturePolicySha256"] == manifest["capturePolicySha256"]
        assert exported["resources"] == manifest["resources"]
        assert [artifact["format"] for artifact in exported["artifacts"]] == [
            "pdf",
            "markdown",
            "zip",
            "self-extracting-html",
            "mhtml",
        ]
        for artifact in exported["artifacts"]:
            verified = await offprint.artifacts.verify_format(
                artifact["path"],
                artifact["format"],
            )
            assert verified["sha256"] == artifact["sha256"]

        request = offprint.captures.request(source_url, output=cancelled_output)
        request["readiness"]["delay"] = 30_000
        job = await offprint.captures.start(request)
        events = job.events()
        saw_started = False
        while True:
            event = await asyncio.wait_for(
                anext(events),
                timeout=ASYNC_OPERATION_TIMEOUT,
            )
            saw_started = saw_started or event["type"] == "capture.started"
            if event["type"] == "navigation.started":
                job.cancel()
                break
        assert saw_started
        with pytest.raises(ShutdownError) as captured:
            await asyncio.wait_for(
                job.result(),
                timeout=ASYNC_OPERATION_TIMEOUT,
            )
        assert captured.value.code == "offprint.runtime.cancelled"

        terminal = None
        while True:
            try:
                event = await asyncio.wait_for(
                    anext(events),
                    timeout=ASYNC_OPERATION_TIMEOUT,
                )
            except StopAsyncIteration:
                break
            if event["type"] == "capture.cancelled":
                terminal = event
        assert terminal is not None
        assert terminal["captureId"] == job.id
        assert not cancelled_output.exists()
    finally:
        await offprint.close()
        await offprint.close()


@pytest.mark.asyncio
async def test_configured_request_captures_rendered_html_to_bounded_memory(
    source_url: str,
) -> None:
    async with Offprint() as offprint:
        request = offprint.captures.request(source_url, selector="#capture")
        request["output"] = {"kind": "memory", "maxBytes": 1024 * 1024}
        job = await offprint.captures.start(request)
        receipt = await asyncio.wait_for(job.result(), timeout=30)

        assert receipt["artifact"]["kind"] == "bytes"
        html = bytes(receipt["artifact"]["content"]).decode("utf-8")
        assert "binding rendered" in html
        assert receipt["artifact"]["bytes"] <= 1024 * 1024
        assert receipt["verification"]["mode"] == "offline"
        assert receipt["verification"]["networkRequests"] == 0
