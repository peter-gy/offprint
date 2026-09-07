import { readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const bindingDirectory = fileURLToPath(new URL("../", import.meta.url));
const source = fileURLToPath(new URL("../../../LICENSE", import.meta.url));

const destination = new URL("../LICENSE", import.meta.url);

export async function syncLicense() {
  const license = await readFile(source);
  await writeFile(destination, license);
}

export async function cleanLicense() {
  const license = await readFile(source);
  try {
    const current = await readFile(destination);
    if (current.equals(license)) {
      await rm(destination);
    }
  } catch (error) {
    if (error.code !== "ENOENT") {
      throw error;
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
