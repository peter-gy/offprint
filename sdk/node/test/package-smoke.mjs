import { execFile } from "node:child_process";
import { createRequire } from "node:module";
import { promisify } from "node:util";
import { strict as assert } from "node:assert";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { Offprint } from "offprint";

const execute = promisify(execFile);
const cli = join(dirname(createRequire(import.meta.url).resolve("offprint")), "cli.cjs");
const command = (...arguments_) =>
  execute(process.execPath, [cli, ...arguments_], { timeout: 120_000 });
const version = await command("--version");
assert.match(version.stdout, /^offprint \d+\.\d+\.\d+\r?\n$/u);
assert.equal(version.stderr, "");
const help = await command("capture", "--help");
assert.match(help.stdout, /--output/u);
assert.equal(help.stderr, "");
await assert.rejects(command("capture", "--json"), (error) => {
  assert.equal(error.code, 2);
  assert.equal(error.stdout, "");
  assert.equal(JSON.parse(error.stderr).code, "offprint.input.arguments");
  return true;
});

const directory = await mkdtemp(join(tmpdir(), "offprint-node-package-"));
let onSlowRequest;
const server = createServer((request, response) => {
  if (request.url === "/slow") {
    response.writeHead(200, { "content-type": "text/html" });
    response.write("<!doctype html><title>Pending navigation</title>");
    onSlowRequest?.();
    return;
  }
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
  const cliOutput = join(directory, "CLI capture ü.html");
  const captured = await command(
    "capture",
    `http://127.0.0.1:${address.port}/`,
    "-o",
    cliOutput,
    "--json",
    ...(browserPath ? ["--browser-path", browserPath] : []),
  );
  assert.equal(JSON.parse(captured.stdout).verification.networkRequests, 0);
  assert.match(await readFile(cliOutput, "utf8"), /installed package rendered/u);
  if (process.platform !== "win32") {
    const waitingForHeaders = execute(
      process.execPath,
      [
        cli,
        "capture",
        "https://example.com",
        "--headers",
        "-",
        "-o",
        join(directory, "headers.html"),
      ],
      { timeout: 10_000 },
    );
    const input = waitingForHeaders.child.stdin;
    // Exceed pipe buffering while staying below the 1 MiB credential limit.
    assert.equal(input.write(Buffer.alloc(768 * 1024, " ")), false);
    await Promise.all([
      once(input, "drain", { signal: AbortSignal.timeout(10_000) }).then(() =>
        waitingForHeaders.child.kill("SIGINT"),
      ),
      assert.rejects(waitingForHeaders, (error) => {
        assert.equal(error.signal, "SIGINT");
        return true;
      }),
    ]);

    const interruptedOutput = join(directory, "interrupted.html");
    let child;
    onSlowRequest = () => child.kill("SIGINT");
    const interrupted = execute(
      process.execPath,
      [
        cli,
        "capture",
        `http://127.0.0.1:${address.port}/slow`,
        "-o",
        interruptedOutput,
        "--json",
        ...(browserPath ? ["--browser-path", browserPath] : []),
      ],
      { timeout: 120_000 },
    );
    child = interrupted.child;
    await assert.rejects(interrupted, (error) => {
      assert.equal(error.code, 130);
      assert.equal(error.stdout, "");
      assert.equal(JSON.parse(error.stderr).code, "offprint.runtime.interrupted");
      return true;
    });
    await assert.rejects(readFile(interruptedOutput), { code: "ENOENT" });
  }
} finally {
  await offprint.close();
  server.closeAllConnections();
  await new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
  await rm(directory, { recursive: true, force: true });
}
