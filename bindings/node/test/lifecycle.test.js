import { strict as assert } from "node:assert";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { test } from "bun:test";

import { findOwnedProcesses } from "./lifecycle-processes.mjs";
import { runLifecycleScenario } from "./lifecycle-runner.mjs";

const childScript = fileURLToPath(
  new URL("./lifecycle-child.mjs", import.meta.url),
);

test("tracks the Chromium process tree from its owned profile", () => {
  const directory = "/pageknot-lifecycle-fixture";
  const processes = [
    {
      pid: 10,
      parentPid: 1,
      name: "chrome.exe",
      commandLine:
        `chrome --user-data-dir=${directory}/pageknot-browser-fixture`,
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
    findOwnedProcesses(directory, processes).map(
      (candidate) => candidate.pid,
    ),
    [10, 11, 12],
  );
});

for (const scenario of [
  "retained-job",
  "explicit-close",
  "abandoned-close",
  "host-exit",
]) {
  test(
    `releases a live browser after ${scenario}`,
    async () => {
      const directory = await mkdtemp(
        join(tmpdir(), `pageknot-node-${scenario}-`),
      );
      try {
        await runLifecycleScenario({
          childScript,
          scenario,
          directory,
        });
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
    80_000,
  );
}
