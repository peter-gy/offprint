import { spawn } from "node:child_process";
import { access, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  ownedProcesses,
  ownedProfiles,
  waitForLifecycleCleanup,
} from "./lifecycle-processes.mjs";

const delay = (duration) =>
  new Promise((resolve) => setTimeout(resolve, duration));

async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

export async function runLifecycleScenario({
  childScript,
  scenario,
  directory,
  environment = {},
  timeout = 60_000,
}) {
  const ready = join(directory, ".pageknot-lifecycle-ready");
  const proceed = join(directory, ".pageknot-lifecycle-continue");
  const output = [];
  const child = spawn(
    process.execPath,
    ["--expose-gc", childScript, scenario],
    {
      env: {
        ...process.env,
        ...environment,
        PAGEKNOT_LIFECYCLE_READY: ready,
        PAGEKNOT_LIFECYCLE_CONTINUE: proceed,
        TEMP: directory,
        TMP: directory,
        TMPDIR: directory,
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  child.stdout.on("data", (chunk) => output.push(chunk));
  child.stderr.on("data", (chunk) => output.push(chunk));

  let exited = false;
  const statusPromise = new Promise((resolve) => {
    child.once("error", (error) => {
      exited = true;
      resolve({ error });
    });
    child.once("exit", (code, signal) => {
      exited = true;
      resolve({ code, signal });
    });
  });
  const deadline = Date.now() + timeout;
  const tracked = [];
  const errors = [];

  try {
    while (!(await pathExists(ready))) {
      if (exited) {
        throw new Error(
          `${scenario} exited before reporting a live browser`,
        );
      }
      if (Date.now() >= deadline) {
        throw new Error(
          `${scenario} timed out before reporting a live browser`,
        );
      }
      await Promise.race([statusPromise, delay(25)]);
    }

    const [processes, profiles] = await Promise.all([
      ownedProcesses(directory),
      ownedProfiles(directory),
    ]);
    if (processes.length === 0) {
      throw new Error(
        `${scenario} did not expose its owned browser process tree`,
      );
    }
    if (profiles.length === 0) {
      throw new Error(
        `${scenario} did not expose its owned browser profile`,
      );
    }
    tracked.push(...processes.map((candidate) => candidate.pid));
    await writeFile(proceed, "continue\n", "utf8");

    const remaining = Math.max(deadline - Date.now(), 1);
    const status = await Promise.race([
      statusPromise,
      delay(remaining).then(() => {
        throw new Error(`${scenario} timed out after browser readiness`);
      }),
    ]);
    if (status.error) {
      throw status.error;
    }
    if (status.code !== 0) {
      throw new Error(
        `${scenario} ended with ${JSON.stringify(status)}`,
      );
    }
  } catch (error) {
    errors.push(error);
  } finally {
    if (!exited) {
      child.kill("SIGKILL");
      await Promise.race([statusPromise, delay(5_000)]);
    }
    try {
      await waitForLifecycleCleanup(directory, tracked);
    } catch (error) {
      errors.push(error);
    }
  }

  if (errors.length > 0) {
    throw new AggregateError(
      errors,
      `${scenario} lifecycle failed\n${Buffer.concat(output).toString("utf8")}`,
    );
  }
}
