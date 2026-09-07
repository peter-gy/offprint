from __future__ import annotations

import asyncio
import json
import os
import signal
import subprocess
import sysconfig
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from offprint import CaptureRequest, CaptureStatus, Offprint, OffprintOptions

slow_request = threading.Event()
finish_request = threading.Event()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/slow":
            self.send_response(200)
            self.send_header("content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<!doctype html><title>Pending navigation</title>")
            self.wfile.flush()
            slow_request.set()
            finish_request.wait(timeout=120)
            return
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


def command(*arguments: str) -> subprocess.CompletedProcess[str]:
    executable = Path(sysconfig.get_path("scripts")) / (
        "offprint.exe" if os.name == "nt" else "offprint"
    )
    assert executable.is_file()
    return subprocess.run([executable, *arguments], capture_output=True, text=True, timeout=120)


async def run() -> None:
    version = command("--version")
    assert version.returncode == 0, version.stderr
    assert version.stdout.startswith("offprint ")
    assert version.stderr == ""
    help_result = command("capture", "--help")
    assert help_result.returncode == 0, help_result.stderr
    assert "--output" in help_result.stdout
    assert help_result.stderr == ""
    invalid = command("capture", "--json")
    assert invalid.returncode == 2
    assert invalid.stdout == ""
    assert json.loads(invalid.stderr)["code"] == "offprint.input.arguments"
    assert CaptureRequest.__name__ == "CaptureRequest"
    assert OffprintOptions.__name__ == "OffprintOptions"
    assert CaptureStatus is not None
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="offprint-wheel-") as directory:
            output = Path(directory) / "capture.html"
            browser_path = os.environ.get("OFFPRINT_PACKAGE_BROWSER_PATH")
            options: OffprintOptions | None = (
                {"browser_path": browser_path} if browser_path else None
            )
            async with Offprint(options) as offprint:
                assert not hasattr(offprint, "_test_panic")
                result = await offprint.capture(
                    f"http://127.0.0.1:{server.server_port}/",
                    output=output,
                )
            assert result["verification"]["networkRequests"] == 0
            artifact = output.read_text(encoding="utf-8")
            assert "installed wheel rendered" in artifact
            assert "data:image/svg+xml;base64," in artifact
            cli_output = Path(directory) / "CLI capture ü.html"
            captured = command(
                "capture",
                f"http://127.0.0.1:{server.server_port}/",
                "-o",
                str(cli_output),
                "--json",
                *(["--browser-path", browser_path] if browser_path else []),
            )
            assert captured.returncode == 0, captured.stderr
            assert json.loads(captured.stdout)["verification"]["networkRequests"] == 0
            assert "installed wheel rendered" in cli_output.read_text(encoding="utf-8")
            if os.name != "nt":
                executable = Path(sysconfig.get_path("scripts")) / "offprint"
                assert executable.is_file()
                interrupted_output = Path(directory) / "interrupted.html"
                with subprocess.Popen(
                    [
                        executable,
                        "capture",
                        f"http://127.0.0.1:{server.server_port}/slow",
                        "-o",
                        str(interrupted_output),
                        "--json",
                        *(["--browser-path", browser_path] if browser_path else []),
                    ],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                ) as child:
                    try:
                        assert slow_request.wait(timeout=120), "CLI did not start navigation"
                        child.send_signal(signal.SIGINT)
                        stdout, stderr = child.communicate(timeout=30)
                        assert child.returncode == 130, stderr
                        assert stdout == ""
                        assert json.loads(stderr)["code"] == "offprint.runtime.interrupted"
                        assert not interrupted_output.exists()
                    finally:
                        finish_request.set()
                        if child.poll() is None:
                            child.kill()
                            child.wait(timeout=10)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


asyncio.run(run())
