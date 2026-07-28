import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { gunzipSync } from "node:zlib";

function archiveEntries(bytes) {
  const tar = gunzipSync(bytes);
  const entries = new Map();
  let offset = 0;
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) {
      break;
    }
    const name = header
      .subarray(0, 100)
      .toString("utf8")
      .replace(/\0.*$/u, "");
    const sizeText = header
      .subarray(124, 136)
      .toString("ascii")
      .replace(/\0.*$/u, "")
      .trim();
    const size = Number.parseInt(sizeText || "0", 8);
    assert(Number.isSafeInteger(size));
    const contentOffset = offset + 512;
    entries.set(name, tar.subarray(contentOffset, contentOffset + size));
    offset = contentOffset + Math.ceil(size / 512) * 512;
  }
  return entries;
}

const [rootPath, platformPath, licensePath] = process.argv.slice(2);
assert(rootPath && platformPath && licensePath);
const expectedLicense = await readFile(licensePath);
const root = archiveEntries(await readFile(rootPath));
const platform = archiveEntries(await readFile(platformPath));

assert.deepEqual(root.get("package/LICENSE"), expectedLicense);
assert(root.has("package/contracts.generated.d.ts"));
assert(root.has("package/native.d.ts"));
assert.equal(
  root.get("package/native.d.ts").includes(Buffer.from("testPanic")),
  false,
);
assert.equal(
  [...root.keys()].filter((name) => name.endsWith(".node")).length,
  0,
);

assert.deepEqual(platform.get("package/LICENSE"), expectedLicense);
assert.equal(
  [...platform.keys()].filter((name) => name.endsWith(".node")).length,
  1,
);
