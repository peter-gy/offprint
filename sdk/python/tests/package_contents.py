from __future__ import annotations

import configparser
import sys
import tarfile
import zipfile
from email.parser import BytesParser
from pathlib import Path


def check_wheel(path: Path, notices: dict[str, bytes]) -> None:
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
        metadata_path = next(
            name for name in archive.namelist() if name.endswith(".dist-info/METADATA")
        )
        metadata = BytesParser().parsebytes(archive.read(metadata_path))
        assert set(metadata.get_all("License-File", [])) == set(notices)
        for name, content in notices.items():
            candidates = [
                entry
                for entry in archive.namelist()
                if entry.endswith(f".dist-info/licenses/{name}")
            ]
            assert len(candidates) == 1, candidates
            assert archive.read(candidates[0]) == content


def check_sdist(path: Path, notices: dict[str, bytes]) -> None:
    with tarfile.open(path, "r:gz") as archive:
        for name, content in notices.items():
            candidates = [
                member
                for member in archive.getmembers()
                if member.isfile() and Path(member.name).name == name
            ]
            assert candidates, name
            for candidate in candidates:
                extracted = archive.extractfile(candidate)
                assert extracted is not None
                assert extracted.read() == content


license_directory = Path(sys.argv[1]).parent
notices = {
    name: (license_directory / name).read_bytes() for name in ("LICENSE", "THIRD_PARTY_NOTICES.txt")
}
archives = [Path(argument) for argument in sys.argv[2:]]
assert archives
for archive in archives:
    if archive.suffix == ".whl":
        check_wheel(archive, notices)
    elif archive.name.endswith(".tar.gz"):
        check_sdist(archive, notices)
    else:
        raise AssertionError(f"unexpected distribution: {archive}")
