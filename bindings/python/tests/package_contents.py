from __future__ import annotations

import sys
import tarfile
import zipfile
from pathlib import Path


def wheel_license(path: Path) -> bytes:
    with zipfile.ZipFile(path) as archive:
        candidates = [
            name
            for name in archive.namelist()
            if name.endswith("/LICENSE") or name == "LICENSE"
        ]
        assert len(candidates) == 1, candidates
        return archive.read(candidates[0])


def sdist_licenses(path: Path) -> list[bytes]:
    with tarfile.open(path, "r:gz") as archive:
        candidates = [
            member
            for member in archive.getmembers()
            if member.isfile() and Path(member.name).name == "LICENSE"
        ]
        assert candidates
        contents = []
        for candidate in candidates:
            extracted = archive.extractfile(candidate)
            assert extracted is not None
            contents.append(extracted.read())
        return contents


license_text = Path(sys.argv[1]).read_bytes()
archives = [Path(argument) for argument in sys.argv[2:]]
assert archives
for archive in archives:
    if archive.suffix == ".whl":
        assert wheel_license(archive) == license_text
    elif archive.name.endswith(".tar.gz"):
        assert all(
            content == license_text for content in sdist_licenses(archive)
        )
    else:
        raise AssertionError(f"unexpected distribution: {archive}")
