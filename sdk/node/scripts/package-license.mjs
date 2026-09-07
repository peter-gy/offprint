import { readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const bindingDirectory = fileURLToPath(new URL("../", import.meta.url));
const files = ["LICENSE", "THIRD_PARTY_NOTICES.txt"];

export async function syncLicense() {
  for (const name of files) {
    const content = await readFile(new URL(`../../../${name}`, import.meta.url));
    await writeFile(new URL(`../${name}`, import.meta.url), content);
  }
}

export async function cleanLicense() {
  for (const name of files) {
    const content = await readFile(new URL(`../../../${name}`, import.meta.url));
    const destination = new URL(`../${name}`, import.meta.url);
    try {
      const current = await readFile(destination);
      if (current.equals(content)) {
        await rm(destination);
      }
    } catch (error) {
      if (error.code !== "ENOENT") {
        throw error;
      }
    }
  }
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  const command = process.argv[2];
  if (command === "sync") {
    await syncLicense();
  } else if (command === "clean") {
    await cleanLicense();
  } else {
    throw new Error(`usage: node ${bindingDirectory}scripts/package-license.mjs <sync|clean>`);
  }
}
