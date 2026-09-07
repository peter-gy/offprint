import { execFile } from "node:child_process";
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

async function windowsProcesses() {
  const script = [
    "$ErrorActionPreference = 'Stop'",
    "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)",
    "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine," +
      "@{Name='CreationTime';Expression={" +
      "if ($null -ne $_.CreationDate) { $_.CreationDate.ToUniversalTime().ToString('o') } " +
      "else { '' }}} | ConvertTo-Json -Compress",
  ].join("; ");
  const { stdout } = await execFileAsync(
    "powershell.exe",
    ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
    {
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
      windowsHide: true,
      timeout: 15_000,
    },
  );
  if (!stdout.trim()) {
    return [];
  }
  const decoded = JSON.parse(stdout);
  const records = Array.isArray(decoded) ? decoded : [decoded];
  return records
    .filter(
      (record) => Number.isInteger(record.ProcessId) && Number.isInteger(record.ParentProcessId),
    )
    .map((record) => ({
      pid: record.ProcessId,
      parentPid: record.ParentProcessId,
      name: typeof record.Name === "string" ? record.Name : "",
      commandLine: typeof record.CommandLine === "string" ? record.CommandLine : "",
      creationTime: typeof record.CreationTime === "string" ? record.CreationTime : "",
    }));
}

async function unixProcesses() {
  const { stdout } = await execFileAsync("ps", ["-axww", "-o", "pid=,ppid=,command="], {
    timeout: 15_000,
  });
  return stdout
    .split("\n")
    .map((line) => line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/))
    .filter((match) => match !== null)
    .map((match) => ({
      pid: Number(match[1]),
      parentPid: Number(match[2]),
      name: "",
      commandLine: match[3],
    }));
}

function isBrowser(process) {
  const identity = `${process.name} ${process.commandLine}`.toLowerCase();
  return ["chrome", "chromium", "msedge", "microsoft-edge", "microsoft edge"].some((name) =>
    identity.includes(name),
  );
}

export function findOwnedProcesses(directory, processes, trackedProcesses = []) {
  const marker = process.platform === "win32" ? directory.toLowerCase() : directory;
  let owned = new Set(
    processes
      .filter((candidate) => {
        const commandLine =
          process.platform === "win32"
            ? candidate.commandLine.toLowerCase()
            : candidate.commandLine;
        return isBrowser(candidate) && commandLine.includes(marker);
      })
      .map((candidate) => candidate.pid),
  );
  const tracked = new Map(trackedProcesses.map((candidate) => [candidate.pid, candidate]));
  const byPid = new Map(processes.map((candidate) => [candidate.pid, candidate]));
  for (const candidate of processes) {
    const previous = tracked.get(candidate.pid);
    if (
      previous &&
      (previous.creationTime && candidate.creationTime
        ? previous.creationTime === candidate.creationTime
        : previous.name === candidate.name && previous.commandLine === candidate.commandLine)
    ) {
      owned.add(candidate.pid);
    }
  }
  for (;;) {
    const expanded = new Set(owned);
    for (const candidate of processes) {
      if (owned.has(candidate.parentPid)) {
        const parent = byPid.get(candidate.parentPid);
        if (!parent) {
          continue;
        }
        // Windows preserves a dead parent's PID after that PID is reused.
        if (parent.creationTime !== undefined || candidate.creationTime !== undefined) {
          if (!parent.creationTime || !candidate.creationTime) {
            continue;
          }
          if (parent.creationTime > candidate.creationTime) {
            continue;
          }
        }
        expanded.add(candidate.pid);
      }
    }
    if (expanded.size === owned.size) {
      break;
    }
    owned = expanded;
  }
  return processes.filter((candidate) => owned.has(candidate.pid));
}

async function systemProcesses() {
  return process.platform === "win32" ? await windowsProcesses() : await unixProcesses();
}

export async function ownedProcesses(directory, trackedProcesses = []) {
  const processes = await systemProcesses();
  return findOwnedProcesses(directory, processes, trackedProcesses);
}

export async function ownedProfiles(directory) {
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") {
      return [];
    }
    throw error;
  }
  return entries
    .filter((entry) => entry.name.startsWith("offprint-browser-"))
    .map((entry) => join(directory, entry.name))
    .sort();
}

export async function waitForLifecycleCleanup(directory, trackedProcesses = []) {
  let processes = [];
  let profiles = [];
  const deadline = Date.now() + 10_000;
  for (;;) {
    [processes, profiles] = await Promise.all([
      ownedProcesses(directory, trackedProcesses),
      ownedProfiles(directory),
    ]);
    if (processes.length === 0 && profiles.length === 0) {
      return;
    }
    if (Date.now() >= deadline) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  const processSummary = processes.map(
    (candidate) => `${candidate.pid}:${candidate.name || candidate.commandLine}`,
  );
  throw new Error(
    `Offprint lifecycle residue under ${directory}: ` +
      `processes=${JSON.stringify(processSummary)}, ` +
      `profiles=${JSON.stringify(profiles)}`,
  );
}
