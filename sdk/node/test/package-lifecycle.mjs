import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { runLifecycleScenario } from "./lifecycle-runner.mjs";

const childScript = fileURLToPath(new URL("./lifecycle-child.mjs", import.meta.url));
const fixture = fileURLToPath(new URL("./capture-request.json", import.meta.url));
const directory = await mkdtemp(join(tmpdir(), "offprint-node-"));
try {
  await runLifecycleScenario({
    childScript,
    scenario: "host-exit",
    directory,
    environment: {
      OFFPRINT_CAPTURE_REQUEST: fixture,
      OFFPRINT_LIFECYCLE_INSTALLED: "1",
    },
  });
} finally {
  await rm(directory, { recursive: true, force: true });
}
