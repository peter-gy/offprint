import { strict as assert } from "node:assert";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { Offprint } from "@offprint/node";

const directory = await mkdtemp(join(tmpdir(), "offprint-node-package-"));
const server = createServer((request, response) => {
  if (request.url === "/asset.svg") {
    const body =
      '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="rgb(24,96,160)"/></svg>';
    response.writeHead(200, {
      "content-type": "image/svg+xml",
      "content-length": Buffer.byteLength(body),
    });
    response.end(body);
    return;
  }
  const body =
    '<!doctype html><title>Installed package</title><h1 id="state">waiting</h1><img src="/asset.svg"><script>document.querySelector("#state").textContent="installed package rendered"</script>';
  response.writeHead(200, {
    "content-type": "text/html; charset=utf-8",
    "content-length": Buffer.byteLength(body),
  });
  response.end(body);
});
await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

const address = server.address();
assert(address && typeof address === "object");
const output = join(directory, "capture.html");
const browserPath = process.env.OFFPRINT_PACKAGE_BROWSER_PATH;
const offprint = new Offprint(browserPath ? { browserPath } : undefined);

try {
  assert.equal("_testPanic" in offprint, false);
  const result = await offprint.capture(`http://127.0.0.1:${address.port}/`, {
    output,
  });
  assert.equal(result.verification.networkRequests, 0);
  const artifact = await readFile(output, "utf8");
  assert.match(artifact, /installed package rendered/);
  assert.match(artifact, /data:image\/svg\+xml;base64,/);
} finally {
  await offprint.close();
  await new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
  await rm(directory, { recursive: true, force: true });
}
