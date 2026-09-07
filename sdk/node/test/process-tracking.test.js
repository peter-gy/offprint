import { strict as assert } from "node:assert";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";

import { test, vi } from "vitest";

import { findOwnedProcesses, ownedProcesses } from "./lifecycle-processes.mjs";

test("tracks the Chromium process tree from its owned profile", () => {
  const directory = "/offprint-lifecycle-fixture";
  const processes = [
    {
      pid: 10,
      parentPid: 1,
      name: "chrome.exe",
      commandLine: `chrome --user-data-dir=${directory}/offprint-browser-fixture`,
    },
    {
      pid: 11,
      parentPid: 10,
      name: "chrome.exe",
      commandLine: "chrome --type=renderer",
    },
    {
      pid: 12,
      parentPid: 11,
      name: "crashpad_handler",
      commandLine: "crashpad_handler",
    },
    {
      pid: 20,
      parentPid: 1,
      name: "chrome.exe",
      commandLine: "chrome --user-data-dir=/unrelated",
    },
  ];

  assert.deepEqual(
    findOwnedProcesses(directory, processes).map((candidate) => candidate.pid),
    [10, 11, 12],
  );
});

test("observes a live process tree after a long command line", async () => {
  const directory = await mkdtemp(join(tmpdir(), "offprint-node-"));
  vi.stubEnv("COLUMNS", "40");
  const child = spawn(process.execPath, [
    "-e",
    `const { spawn } = require("node:child_process");
    const child = spawn(process.execPath, ["-e", "process.stdin.resume()"], {
      stdio: ["pipe", "ignore", "inherit"],
    });
    child.once("spawn", () => process.stdout.write(String(child.pid) + "\\n"));
    process.stdin.once("end", () => child.stdin.end());
    process.stdin.resume();
    child.once("exit", (code) => process.exit(code ?? 1));`,
    "--",
    "--chrome-fixture",
    "x".repeat(512),
    `--user-data-dir=${directory}/offprint-browser-fixture`,
  ]);
  const lines = createInterface({ input: child.stdout });
  const exited = once(child, "exit", { signal: AbortSignal.timeout(30_000) });
  try {
    const [line] = await once(lines, "line", { signal: AbortSignal.timeout(10_000) });
    const descendantPid = Number(line);
    assert(Number.isInteger(descendantPid) && descendantPid > 0);
    const records = new Map(
      (await ownedProcesses(directory)).map((record) => [record.pid, record]),
    );
    assert(records.has(child.pid));
    assert.equal(records.get(descendantPid).parentPid, child.pid);
    if (process.platform === "win32") {
      assert(records.get(child.pid).creationTime);
      assert(records.get(descendantPid).creationTime);
      assert(records.get(child.pid).creationTime <= records.get(descendantPid).creationTime);
    }
  } finally {
    child.stdin.end();
    try {
      const [code] = await exited;
      assert.equal(code, 0);
    } finally {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
      }
      lines.close();
      vi.unstubAllEnvs();
      await rm(directory, { recursive: true, force: true });
    }
  }
}, 40_000);

test("rejects Windows parent PID reuse by processes older than the browser", () => {
  const directory = "/offprint-lifecycle-fixture";
  const processes = [
    {
      pid: 10,
      parentPid: 1,
      name: "chrome.exe",
      commandLine: `chrome --user-data-dir=${directory}/offprint-browser-fixture`,
      creationTime: "2026-09-07T12:00:00.0000000Z",
    },
    {
      pid: 20,
      parentPid: 10,
      name: "services.exe",
      commandLine: "services.exe",
      creationTime: "2026-09-07T10:00:00.0000000Z",
    },
    {
      pid: 21,
      parentPid: 20,
      name: "service.exe",
      commandLine: "service.exe",
      creationTime: "2026-09-07T10:00:01.0000000Z",
    },
    {
      pid: 11,
      parentPid: 10,
      name: "chrome.exe",
      commandLine: "chrome --type=renderer",
      creationTime: "2026-09-07T12:00:00.0000001Z",
    },
    {
      pid: 12,
      parentPid: 11,
      name: "crashpad_handler",
      commandLine: "crashpad_handler",
      creationTime: "2026-09-07T12:00:00.0000002Z",
    },
  ];

  assert.deepEqual(
    findOwnedProcesses(directory, processes).map((candidate) => candidate.pid),
    [10, 11, 12],
  );
});

test("rejects a reused PID when matching tracked process identity", () => {
  const previous = {
    pid: 12,
    parentPid: 11,
    name: "crashpad_handler",
    commandLine: "crashpad_handler",
    creationTime: "2026-09-07T12:00:00.0000001Z",
  };
  const reused = { ...previous, creationTime: "2026-09-07T12:00:00.0000002Z" };

  assert.deepEqual(findOwnedProcesses("/offprint-lifecycle-fixture", [reused], [previous]), []);
});

test("retains a reparented crashpad with its tracked process identity", () => {
  const previous = {
    pid: 12,
    parentPid: 11,
    name: "crashpad_handler",
    commandLine: "crashpad_handler",
    creationTime: "2026-09-07T12:00:00.0000001Z",
  };
  const survivor = { ...previous, parentPid: 1, commandLine: "crashpad_handler --monitor-self" };

  assert.deepEqual(findOwnedProcesses("/offprint-lifecycle-fixture", [survivor], [previous]), [
    survivor,
  ]);
});
