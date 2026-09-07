from __future__ import annotations

import argparse
import copy
import gzip
import io
import json
import os
import subprocess
import tarfile
import tempfile
from pathlib import Path
from typing import Any


def metadata(manifest: Path, *, locked: bool) -> dict[str, Any]:
    command = [
        "cargo",
        "metadata",
        "--manifest-path",
        str(manifest),
        "--format-version",
        "1",
        "--offline",
    ]
    if locked:
        command.append("--locked")
    result = subprocess.run(command, check=True, stdout=subprocess.PIPE, text=True, timeout=120)
    return json.loads(result.stdout)


def registry_packages(document: dict[str, Any]) -> set[tuple[str, str, str]]:
    return {
        (package["name"], package["version"], package["source"])
        for package in document["packages"]
        if package["source"] is not None
    }


def prepare(archive: Path) -> None:
    source_manifest = Path(__file__).resolve().parents[3] / "offprint-rs" / "Cargo.toml"
    original = registry_packages(metadata(source_manifest, locked=True))
    with tempfile.TemporaryDirectory(prefix=".offprint-sdist-", dir=archive.parent) as temporary:
        staging = Path(temporary)
        with tarfile.open(archive, "r:gz") as source:
            source.extractall(staging, filter="data")
        roots = list(staging.iterdir())
        if len(roots) != 1 or not roots[0].is_dir():
            raise ValueError("source distribution must contain one root directory")
        extracted_manifest = roots[0] / "offprint-rs" / "Cargo.toml"
        lock = extracted_manifest.with_name("Cargo.lock")
        if lock.read_bytes() != source_manifest.with_name("Cargo.lock").read_bytes():
            raise ValueError("source distribution must contain the workspace lockfile")

        # Maturin prunes workspace members but copies Cargo.lock unchanged.
        normalized = registry_packages(metadata(extracted_manifest, locked=False))
        changed = normalized - original
        if changed:
            raise ValueError(f"source distribution changed locked dependencies: {sorted(changed)}")
        metadata(extracted_manifest, locked=True)
        replacement = lock.read_bytes()
        lock_member = lock.relative_to(staging).as_posix()
        output = staging / archive.name
        epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
        with (
            tarfile.open(archive, "r:gz") as source,
            output.open("wb") as destination,
            gzip.GzipFile(filename="", mode="wb", fileobj=destination, mtime=epoch) as compressed,
            tarfile.open(fileobj=compressed, mode="w") as packed,
        ):
            for member in source.getmembers():
                entry = copy.copy(member)
                if member.name == lock_member:
                    entry.size = len(replacement)
                    packed.addfile(entry, io.BytesIO(replacement))
                else:
                    packed.addfile(entry, source.extractfile(member) if member.isfile() else None)
        os.replace(output, archive)
    print(f"Prepared {archive.name} with {len(normalized)} locked registry dependencies")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    prepare(parser.parse_args().archive.resolve())
