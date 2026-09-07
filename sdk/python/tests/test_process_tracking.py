from __future__ import annotations

import json
import subprocess
import sys
import threading
from dataclasses import replace
from pathlib import Path

import pytest
from lifecycle_support import ProcessRecord, _owned_processes, owned_processes


def test_owned_browser_processes_include_the_profile_tree(
    tmp_path: Path,
) -> None:
    processes = [
        ProcessRecord(
            pid=10,
            parent_pid=1,
            name="chrome.exe",
            command_line=(f"chrome --user-data-dir={tmp_path}/offprint-browser-fixture"),
        ),
        ProcessRecord(
            pid=11,
            parent_pid=10,
            name="chrome.exe",
            command_line="chrome --type=renderer",
        ),
        ProcessRecord(
            pid=12,
            parent_pid=11,
            name="crashpad_handler",
            command_line="crashpad_handler",
        ),
        ProcessRecord(
            pid=20,
            parent_pid=1,
            name="chrome.exe",
            command_line="chrome --user-data-dir=/unrelated",
        ),
    ]

    owned = _owned_processes(tmp_path, processes)

    assert [process.pid for process in owned] == [10, 11, 12]


def test_observes_a_live_process_tree_after_a_long_command_line(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("COLUMNS", "40")
    script = """
import json
import os
import subprocess
import sys

child = subprocess.Popen(
    [sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
    stdin=subprocess.PIPE,
)
try:
    print(json.dumps({"parent": os.getpid(), "child": child.pid}), flush=True)
    sys.stdin.buffer.read(1)
finally:
    child.stdin.close()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=5)
"""
    process = subprocess.Popen(
        [
            sys.executable,
            "-c",
            script,
            "--chrome-fixture",
            "x" * 512,
            f"--user-data-dir={tmp_path}/offprint-browser-fixture",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )
    assert process.stdin is not None
    assert process.stdout is not None
    ready = threading.Event()
    lines: list[str] = []
    output = process.stdout

    def read_ready() -> None:
        lines.append(output.readline())
        ready.set()

    reader = threading.Thread(target=read_ready, daemon=True)
    reader.start()
    try:
        assert ready.wait(timeout=10), "Process did not report its child"
        reported = json.loads(lines[0])
        parent_pid = reported["parent"]
        descendant_pid = reported["child"]
        records = {record.pid: record for record in owned_processes(tmp_path)}
        assert process.pid in records
        assert records[descendant_pid].parent_pid == parent_pid
        if sys.platform == "win32":
            parent_creation = records[parent_pid].creation_time
            child_creation = records[descendant_pid].creation_time
            assert parent_creation and child_creation
            assert parent_creation <= child_creation
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=10)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            reader.join(timeout=5)
            process.stdout.close()
        assert process.returncode == 0


def test_windows_parent_pid_reuse_excludes_processes_older_than_the_browser(
    tmp_path: Path,
) -> None:
    browser = ProcessRecord(
        10,
        1,
        "chrome.exe",
        f"chrome --user-data-dir={tmp_path}/offprint-browser-fixture",
        "2026-09-07T12:00:00.0000000Z",
    )
    processes = [
        browser,
        ProcessRecord(20, 10, "services.exe", "services.exe", "2026-09-07T10:00:00.0000000Z"),
        ProcessRecord(21, 20, "service.exe", "service.exe", "2026-09-07T10:00:01.0000000Z"),
        ProcessRecord(
            11, 10, "chrome.exe", "chrome --type=renderer", "2026-09-07T12:00:00.0000001Z"
        ),
        ProcessRecord(
            12, 11, "crashpad_handler", "crashpad_handler", "2026-09-07T12:00:00.0000002Z"
        ),
    ]

    assert [record.pid for record in _owned_processes(tmp_path, processes)] == [10, 11, 12]


def test_tracked_process_identity_rejects_a_reused_pid(tmp_path: Path) -> None:
    previous = ProcessRecord(
        12, 11, "crashpad_handler", "crashpad_handler", "2026-09-07T12:00:00.0000001Z"
    )
    reused = replace(previous, creation_time="2026-09-07T12:00:00.0000002Z")

    assert _owned_processes(tmp_path, [reused], [previous]) == []


def test_tracked_process_identity_retains_a_reparented_crashpad(tmp_path: Path) -> None:
    previous = ProcessRecord(
        12, 11, "crashpad_handler", "crashpad_handler", "2026-09-07T12:00:00.0000001Z"
    )
    survivor = replace(previous, parent_pid=1, command_line="crashpad_handler --monitor-self")

    assert _owned_processes(tmp_path, [survivor], [previous]) == [survivor]
