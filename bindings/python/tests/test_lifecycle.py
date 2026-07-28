from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

from lifecycle_support import (
    ProcessRecord,
    _owned_processes,
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
            command_line=(
                f"chrome --user-data-dir={tmp_path}/pageknot-browser-fixture"
            ),
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
    tmp_path: Path,
) -> None:
    directory = tmp_path / scenario
    directory.mkdir()
    environment = {
        **os.environ,
        "TEMP": str(directory),
        "TMP": str(directory),
        "TMPDIR": str(directory),
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
