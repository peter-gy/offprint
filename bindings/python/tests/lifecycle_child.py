from __future__ import annotations

import asyncio
import gc
import json
import os
import sys
import threading
import weakref
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from offprint import Offprint


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        body = (
            b"<!doctype html><title>Lifecycle</title>"
            b"<h1>active capture</h1>"
        )
        self.send_response(200)
        self.send_header("content-type", "text/html; charset=utf-8")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


async def run(scenario: str) -> Offprint | None:
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        fixture_path = Path(
            os.environ.get(
                "OFFPRINT_CAPTURE_REQUEST",
                Path(__file__).parents[3]
                / "schemas"
                / "examples"
                / "capture-request.json",
            )
        )
        request = json.loads(fixture_path.read_text(encoding="utf-8"))
        request["url"] = f"http://127.0.0.1:{server.server_port}/"
        request["output"]["path"] = str(
            Path(os.environ["TMPDIR"]) / f"{scenario}.html"
        )
        request["readiness"]["delay"] = 30_000
        browser_path = os.environ.get("OFFPRINT_PACKAGE_BROWSER_PATH")
        options = {"browser_path": browser_path} if browser_path else None
        offprint: Offprint | None = Offprint(options)
        job = await offprint.captures.start(request)
        events = job.events()
        async for event in events:
            if event["type"] == "navigation.started":
                break

        ready_path = os.environ.get("OFFPRINT_LIFECYCLE_READY")
        continue_path = os.environ.get("OFFPRINT_LIFECYCLE_CONTINUE")
        if ready_path is not None and continue_path is not None:
            Path(ready_path).write_text("ready\n", encoding="utf-8")
            while not Path(continue_path).exists():
                await asyncio.sleep(0.01)

        if scenario == "retained-job":
            reference = weakref.ref(offprint)
            offprint = None
            gc.collect()
            owner = reference()
            assert owner is not None
            job.cancel()
            try:
                await job.result()
            except BaseException:
                pass
            await owner.close()
        elif scenario == "explicit-close":
            await offprint.close()
        elif scenario == "abandoned-close":
            asyncio.create_task(offprint.close())
            offprint = None
            gc.collect()
            await asyncio.sleep(0)
        elif scenario in {"host-exit", "collected-after-loop-stop"}:
            return offprint
        return None
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


scenario = sys.argv[1]
assert scenario in {
    "retained-job",
    "explicit-close",
    "abandoned-close",
    "host-exit",
    "collected-after-loop-stop",
}
offprint = asyncio.run(run(scenario))
if scenario == "collected-after-loop-stop":
    reference = weakref.ref(offprint)
    offprint = None
    gc.collect()
    gc.collect()
    assert reference() is None
    assert not list(Path(os.environ["TMPDIR"]).glob("offprint-browser-*"))
