from __future__ import annotations

import asyncio
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

import pytest

from pageknot import PageKnot, ShutdownError


EXAMPLES = Path(__file__).parents[3] / "schemas" / "examples"
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


def capture_request(url: str, output: Path) -> dict[str, Any]:
    value = json.loads(
        (EXAMPLES / "capture-request.json").read_text(encoding="utf-8")
    )
    assert isinstance(value, dict)
    value["url"] = url
    value["artifact"]["options"]["target"]["value"] = str(output)
    return value


@pytest.mark.asyncio
async def test_capture_event_cancellation_and_close_contract(
    source_url: str,
    tmp_path: Path,
) -> None:
    output = tmp_path / "python-binding.html"
    cancelled_output = tmp_path / "python-cancelled.html"
    pageknot = PageKnot()

    try:
        output.write_text("stale artifact", encoding="utf-8")
        result = await pageknot.capture(
            source_url,
            output=output,
            selector="#capture",
        )

        assert result["schemaVersion"] == 1
        assert result["status"] == "succeeded"
        assert result["artifact"]["kind"] == "file"
        assert result["artifact"]["path"] == str(output)
        assert result["verification"]["passed"] is True
        assert result["verification"]["networkRequests"] == 0
        content = output.read_text(encoding="utf-8")
        assert "binding rendered" in content
        assert "data:image/svg+xml;base64," in content
        assert "outside selector" not in content
        assert "stale artifact" not in content

        manifest = await pageknot.artifacts.inspect(output)
        assert manifest["schemaVersion"] == 1
        assert manifest["source"]["finalUrl"] == source_url
        assert manifest["resources"]["embedded"] >= 1

        exported = await pageknot.artifacts.export(
            output,
            {
                "outputDirectory": str(tmp_path / "python-variants"),
                "baseName": "python-binding",
                "conflict": "fail",
                "variants": [
                    {"kind": "pdf", "options": {}},
                    {
                        "kind": "markdown",
                        "options": {"frontMatter": True},
                    },
                    {"kind": "zip"},
                    {"kind": "self-extracting"},
                    {"kind": "mhtml"},
                ],
            },
        )
        assert exported["policySha256"] == manifest["policySha256"]
        assert exported["resources"] == manifest["resources"]
        assert [
            variant["kind"] for variant in exported["variants"]
        ] == [
            "pdf",
            "markdown",
            "zip",
            "self-extracting",
            "mhtml",
        ]
        for variant in exported["variants"]:
            verified = await pageknot.artifacts.verify_variant(
                variant["path"],
                variant["kind"],
            )
            assert verified["passed"] is True
            assert verified["sha256"] == variant["sha256"]

        request = capture_request(source_url, cancelled_output)
        request["readiness"]["delay"] = 30_000
        job = await pageknot.captures.start(request)
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
        assert captured.value.code == "pageknot.runtime.cancelled"

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
        await pageknot.close()
        await pageknot.close()
