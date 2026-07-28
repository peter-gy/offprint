import { strict as assert } from "node:assert";
import { access, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";

const pageknotModule =
  process.env.PAGEKNOT_LIFECYCLE_INSTALLED === "1"
    ? await import("@pageknot/node")
    : await import("../index.js");
const { PageKnot } = pageknotModule;

const scenario = process.argv[2];
assert(
  ["retained-job", "explicit-close", "abandoned-close", "host-exit"].includes(
    scenario,
  ),
);

const server = createServer((_request, response) => {
  const body =
    "<!doctype html><title>Lifecycle</title><h1>active capture</h1>";
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
const fixturePath =
  process.env.PAGEKNOT_CAPTURE_REQUEST ??
  new URL("../../../schemas/examples/capture-request.json", import.meta.url);
const request = JSON.parse(await readFile(fixturePath, "utf8"));
request.url = `http://127.0.0.1:${address.port}/`;
request.artifact.options.target.value = join(
  process.env.TMPDIR,
  `${scenario}.html`,
);
request.readiness.delay = 30_000;

const browserPath = process.env.PAGEKNOT_PACKAGE_BROWSER_PATH;
let pageknot = new PageKnot(browserPath ? { browserPath } : undefined);
const job = await pageknot.captures.start(request);
const events = job.events();
for await (const event of events) {
  if (event.type === "navigation.started") {
    break;
  }
}

const readyPath = process.env.PAGEKNOT_LIFECYCLE_READY;
const continuePath = process.env.PAGEKNOT_LIFECYCLE_CONTINUE;
if (readyPath && continuePath) {
  await writeFile(readyPath, "ready\n", "utf8");
  for (;;) {
    try {
      await access(continuePath);
      break;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
}

if (scenario === "retained-job") {
  const reference = new WeakRef(pageknot);
  pageknot = undefined;
  for (let attempt = 0; attempt < 10; attempt += 1) {
    global.gc();
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  const owner = reference.deref();
  assert(owner);
  job.cancel();
  await job.result().catch(() => {});
  await owner.close();
} else if (scenario === "explicit-close") {
  await pageknot.close();
} else if (scenario === "abandoned-close") {
  void pageknot.close();
  pageknot = undefined;
  global.gc();
}

await new Promise((resolve, reject) => {
  server.close((error) => (error ? reject(error) : resolve()));
});

if (scenario === "host-exit") {
  process.exit(0);
}
