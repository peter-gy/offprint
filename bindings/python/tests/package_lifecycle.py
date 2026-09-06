from __future__ import annotations

import os
import sys
import tempfile
from pathlib import Path

from lifecycle_support import run_lifecycle_child


tests_directory = Path(__file__).resolve().parent
fixture = (
    tests_directory.parents[2]
    / "schemas"
    / "examples"
    / "capture-request.json"
)

with tempfile.TemporaryDirectory(
    prefix="offprint-python-package-host-exit-"
) as temporary_directory:
    directory = Path(temporary_directory)
    inherited_environment = dict(os.environ)
    inherited_environment.pop("PYTHONPATH", None)
    environment = {
        **inherited_environment,
        "OFFPRINT_CAPTURE_REQUEST": str(fixture),
        "PYTHONNOUSERSITE": "1",
        "TEMP": str(directory),
        "TMP": str(directory),
        "TMPDIR": str(directory),
    }
    run_lifecycle_child(
        executable=sys.executable,
        script=tests_directory / "lifecycle_child.py",
        scenario="host-exit",
        cwd=tests_directory.parent,
        environment=environment,
        directory=directory,
        timeout=60,
    )
