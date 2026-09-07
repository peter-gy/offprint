from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

import pytest
from lifecycle_support import run_lifecycle_child


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
