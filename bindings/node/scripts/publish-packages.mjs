import { spawn } from "node:child_process";

import {
  cleanLicense,
  syncLicense,
} from "./package-license.mjs";

await syncLicense();
try {
  const status = await new Promise((resolve, reject) => {
    const child = spawn("napi", ["prepublish", "-t", "npm"], {
      shell: process.platform === "win32",
      stdio: "inherit",
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
  if (status.code !== 0) {
    throw new Error(
      status.signal
        ? `napi prepublish ended after ${status.signal}`
        : `napi prepublish exited with ${status.code}`,
    );
  }
} finally {
  await cleanLicense();
}
