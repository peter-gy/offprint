import { readFile, readdir, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const bindingDirectory = fileURLToPath(new URL("../", import.meta.url));
const source = fileURLToPath(
  new URL("../../../LICENSE", import.meta.url),
);

async function destinations() {
  const npmDirectory = new URL("../npm/", import.meta.url);
  const entries = await readdir(npmDirectory, { withFileTypes: true });
  return [
    new URL("../LICENSE", import.meta.url),
    ...entries
      .filter((entry) => entry.isDirectory())
      .map((entry) => new URL(`../npm/${entry.name}/LICENSE`, import.meta.url)),
  ];
}

export async function syncLicense() {
  const license = await readFile(source);
  await Promise.all(
    (await destinations()).map((destination) =>
      writeFile(destination, license),
    ),
  );
}

export async function cleanLicense() {
  const license = await readFile(source);
  await Promise.all(
    (await destinations()).map(async (destination) => {
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
    }),
  );
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  const command = process.argv[2];
  if (command === "sync") {
    await syncLicense();
  } else if (command === "clean") {
    await cleanLicense();
  } else {
    throw new Error(
      `usage: node ${bindingDirectory}scripts/package-license.mjs <sync|clean>`,
    );
  }
}
