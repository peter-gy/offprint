from __future__ import annotations

import json
import subprocess
import sys
import time
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ProcessRecord:
    pid: int
    parent_pid: int
    name: str
    command_line: str
    creation_time: str | None = None


def _windows_processes() -> list[ProcessRecord]:
    script = (
        "$ErrorActionPreference = 'Stop'; "
        "[Console]::OutputEncoding = "
        "[System.Text.UTF8Encoding]::new($false); "
        "Get-CimInstance Win32_Process | "
        "Select-Object ProcessId,ParentProcessId,Name,CommandLine,"
        "@{Name='CreationTime';Expression={"
        "if ($null -ne $_.CreationDate) { $_.CreationDate.ToUniversalTime().ToString('o') } "
        "else { '' }}} | "
        "ConvertTo-Json -Compress"
    )
    result = subprocess.run(
        [
            "powershell.exe",
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=15,
    )
    if not result.stdout.strip():
        return []
    decoded: object = json.loads(result.stdout)
    records = decoded if isinstance(decoded, list) else [decoded]
    processes: list[ProcessRecord] = []
    for record in records:
        if not isinstance(record, dict):
            continue
        pid = record.get("ProcessId")
        parent_pid = record.get("ParentProcessId")
        if not isinstance(pid, int) or not isinstance(parent_pid, int):
            continue
        name = record.get("Name")
        command_line = record.get("CommandLine")
        creation_time = record.get("CreationTime")
        processes.append(
            ProcessRecord(
                pid=pid,
                parent_pid=parent_pid,
                name=name if isinstance(name, str) else "",
                command_line=(command_line if isinstance(command_line, str) else ""),
                creation_time=creation_time if isinstance(creation_time, str) else "",
            )
        )
    return processes


def _unix_processes() -> list[ProcessRecord]:
    result = subprocess.run(
        ["ps", "-axww", "-o", "pid=,ppid=,command="],
        timeout=15,
        check=True,
        capture_output=True,
        text=True,
    )
    processes: list[ProcessRecord] = []
    for line in result.stdout.splitlines():
        fields = line.strip().split(maxsplit=2)
        if len(fields) != 3:
            continue
        pid, parent_pid, command_line = fields
        if not pid.isdecimal() or not parent_pid.isdecimal():
            continue
        processes.append(
            ProcessRecord(
                pid=int(pid),
                parent_pid=int(parent_pid),
                name="",
                command_line=command_line,
            )
        )
    return processes


def _is_browser(process: ProcessRecord) -> bool:
    identity = f"{process.name} {process.command_line}".casefold()
    return any(
        browser_name in identity
        for browser_name in (
            "chrome",
            "chromium",
            "msedge",
            "microsoft-edge",
            "microsoft edge",
        )
    )


def _descendants(roots: set[int], processes: list[ProcessRecord]) -> set[int]:
    by_pid = {process.pid: process for process in processes}
    owned = roots
    while True:
        expanded = set(owned)
        for process in processes:
            if process.parent_pid not in owned:
                continue
            parent = by_pid.get(process.parent_pid)
            if parent is None:
                continue
            # Windows preserves a dead parent's PID after that PID is reused.
            if parent.creation_time is not None or process.creation_time is not None:
                if not parent.creation_time or not process.creation_time:
                    continue
                if parent.creation_time > process.creation_time:
                    continue
            expanded.add(process.pid)
        if expanded == owned:
            return owned
        owned = expanded


def _owned_processes(
    directory: Path,
    processes: list[ProcessRecord],
    tracked_processes: Iterable[ProcessRecord] = (),
) -> list[ProcessRecord]:
    marker = str(directory)
    if sys.platform == "win32":
        marker = marker.casefold()

    owned = {
        process.pid
        for process in processes
        if _is_browser(process)
        and marker
        in (process.command_line.casefold() if sys.platform == "win32" else process.command_line)
    }
    tracked = {record.pid: record for record in tracked_processes}
    for process in processes:
        previous = tracked.get(process.pid)
        if previous is None:
            continue
        if previous.creation_time and process.creation_time:
            same_process = previous.creation_time == process.creation_time
        else:
            same_process = (
                previous.name == process.name and previous.command_line == process.command_line
            )
        if same_process:
            owned.add(process.pid)
    owned = _descendants(owned, processes)
    return [process for process in processes if process.pid in owned]


def owned_processes(
    directory: Path,
    tracked_processes: Iterable[ProcessRecord] = (),
) -> list[ProcessRecord]:
    processes = _windows_processes() if sys.platform == "win32" else _unix_processes()
    return _owned_processes(directory, processes, tracked_processes)


def owned_profiles(directory: Path) -> list[Path]:
    if not directory.exists():
        return []
    return sorted(directory.glob("offprint-browser-*"))


def _lifecycle_process_snapshot(parent_pid: int, directory: Path) -> str:
    processes = _windows_processes() if sys.platform == "win32" else _unix_processes()
    descendants = _descendants({parent_pid}, processes)
    records = [
        {
            "pid": process.pid,
            "parent_pid": process.parent_pid,
            "browser": _is_browser(process),
            "command_length": len(process.command_line),
            "profile_match": str(directory) in process.command_line,
        }
        for process in processes
        if process.pid in descendants
    ]
    return json.dumps(
        {"processes": records, "profiles": [str(p) for p in owned_profiles(directory)]}
    )


def wait_for_lifecycle_cleanup(
    directory: Path,
    tracked_processes: Iterable[ProcessRecord] = (),
) -> None:
    tracked = tuple(tracked_processes)
    processes: list[ProcessRecord] = []
    profiles: list[Path] = []
    deadline = time.monotonic() + 10
    while True:
        processes = owned_processes(directory, tracked)
        profiles = owned_profiles(directory)
        if not processes and not profiles:
            return
        if time.monotonic() >= deadline:
            break
        time.sleep(0.05)
    process_summary = [
        f"{process.pid}:{process.name or process.command_line}" for process in processes
    ]
    raise RuntimeError(
        f"Offprint lifecycle residue under {directory}: "
        f"processes={process_summary}, profiles={profiles}"
    )


def run_lifecycle_child(
    *,
    executable: str,
    script: Path,
    scenario: str,
    cwd: Path,
    environment: Mapping[str, str],
    directory: Path,
    timeout: float = 60,
) -> None:
    ready = directory / ".offprint-lifecycle-ready"
    proceed = directory / ".offprint-lifecycle-continue"
    child_environment = {
        **environment,
        "OFFPRINT_LIFECYCLE_READY": str(ready),
        "OFFPRINT_LIFECYCLE_CONTINUE": str(proceed),
    }
    process = subprocess.Popen(
        [executable, str(script), scenario],
        cwd=cwd,
        env=child_environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    deadline = time.monotonic() + timeout
    tracked: list[ProcessRecord] = []
    stdout = ""
    stderr = ""
    errors: list[str] = []
    try:
        while not ready.exists():
            if process.poll() is not None:
                stdout, stderr = process.communicate()
                errors.append(f"{scenario} exited before reporting a live browser")
                break
            if time.monotonic() >= deadline:
                errors.append(f"{scenario} timed out before reporting a live browser")
                break
            time.sleep(0.025)

        if not errors:
            tracked = owned_processes(directory)
            profiles = owned_profiles(directory)
            if not tracked:
                errors.append(
                    f"{scenario} did not expose its owned browser process tree: "
                    f"{_lifecycle_process_snapshot(process.pid, directory)}"
                )
            if not profiles:
                errors.append(f"{scenario} did not expose its owned browser profile")

        if not errors:
            proceed.write_text("continue\n", encoding="utf-8")
            remaining = max(deadline - time.monotonic(), 0.001)
            try:
                stdout, stderr = process.communicate(timeout=remaining)
            except subprocess.TimeoutExpired:
                errors.append(f"{scenario} timed out after browser readiness")
    except Exception as error:
        errors.append(f"{scenario} lifecycle observation failed: {error}")
    finally:
        if process.poll() is None:
            process.kill()
            killed_stdout, killed_stderr = process.communicate()
            stdout += killed_stdout
            stderr += killed_stderr
        if process.returncode not in {None, 0} and not errors:
            errors.append(f"{scenario} exited with status {process.returncode}")
        try:
            wait_for_lifecycle_cleanup(
                directory,
                tracked,
            )
        except Exception as error:
            errors.append(str(error))

    if errors:
        raise RuntimeError("\n".join(errors) + f"\nstdout:\n{stdout}\nstderr:\n{stderr}")
