from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest
from lifecycle_support import (
    ProcessRecord,
    _owned_processes,
    owned_processes,
    run_lifecycle_child,
)


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


def test_owned_browser_processes_read_long_command_lines(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("COLUMNS", "40")
    process = subprocess.Popen(
        [
            sys.executable,
            "-c",
            "import time; time.sleep(30)",
            "--chrome-fixture",
            "x" * 512,
            f"--user-data-dir={tmp_path}/offprint-browser-fixture",
        ]
    )
    try:
        assert process.pid in {record.pid for record in owned_processes(tmp_path)}
    finally:
        process.kill()
        process.wait(timeout=5)


def test_lifecycle_child_reports_browser_startup_failure(tmp_path: Path) -> None:
    environment = {
        **os.environ,
        "TEMP": str(tmp_path),
        "TMP": str(tmp_path),
        "TMPDIR": str(tmp_path),
        "OFFPRINT_PACKAGE_BROWSER_PATH": str(tmp_path / "missing-browser"),
    }
    with pytest.raises(RuntimeError, match="browser executable .*missing-browser.*unavailable"):
        run_lifecycle_child(
            executable=sys.executable,
            script=Path(__file__).parent / "lifecycle_child.py",
            scenario="explicit-close",
            cwd=Path(__file__).parent.parent,
            environment=environment,
            directory=tmp_path,
            timeout=10,
        )


@pytest.mark.parametrize(
    "scenario",
    [
        "retained-job",
        "explicit-close",
        "abandoned-close",
        "host-exit",
        "collected-after-loop-stop",
    ],
)
def test_releases_a_live_browser_after_host_lifecycle(
    scenario: str,
) -> None:
    # Chromium's Linux singleton socket must fit in sockaddr_un.sun_path.
    with tempfile.TemporaryDirectory(prefix="offprint-py-") as temporary:
        directory = Path(temporary)
        environment = {
            **os.environ,
            "TEMP": temporary,
            "TMP": temporary,
            "TMPDIR": temporary,
        }
        run_lifecycle_child(
            executable=sys.executable,
            script=Path(__file__).parent / "lifecycle_child.py",
            scenario=scenario,
            cwd=Path(__file__).parent.parent,
            environment=environment,
            directory=directory,
            timeout=60,
        )
