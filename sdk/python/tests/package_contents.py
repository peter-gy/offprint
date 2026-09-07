from __future__ import annotations

import configparser
import sys
import tarfile
import zipfile
from pathlib import Path


def check_wheel(path: Path, license_text: bytes) -> None:
    with zipfile.ZipFile(path) as archive:
        required = {
            "offprint/__init__.py",
            "offprint/_cli.py",
            "offprint/__main__.py",
            "offprint/__init__.pyi",
            "offprint/_native.pyi",
            "offprint/contracts.py",
            "offprint/py.typed",
        }
        assert required <= set(archive.namelist())
        entrypoint = next(
            name for name in archive.namelist() if name.endswith(".dist-info/entry_points.txt")
        )
        parser = configparser.ConfigParser()
        parser.read_string(archive.read(entrypoint).decode("utf-8"))
        assert parser.has_option("console_scripts", "offprint")
        candidates = [
            name for name in archive.namelist() if name.endswith("/LICENSE") or name == "LICENSE"
        ]
        assert len(candidates) == 1, candidates
        assert archive.read(candidates[0]) == license_text


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
        check_wheel(archive, license_text)
    elif archive.name.endswith(".tar.gz"):
        assert all(content == license_text for content in sdist_licenses(archive))
    else:
        raise AssertionError(f"unexpected distribution: {archive}")
