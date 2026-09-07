import { afterAll, beforeAll, expect, test } from "bun:test";
import {
  mkdtemp,
  readFile,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { Offprint, OffprintError } from "../index.js";

let directory;
let server;
let sourceUrl;

beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), "offprint-node-"));
  server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
      const url = new URL(request.url);
      if (url.pathname === "/asset.svg") {
        return new Response(
          `<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
            <rect width="8" height="8" fill="rgb(24, 96, 160)"/>
          </svg>`,
          { headers: { "content-type": "image/svg+xml" } },
        );
      }
      return new Response(
        `<!doctype html><html><head><title>Node binding</title></head>
          <body><p>outside selector</p><main id="capture">
          <h1 id="state">waiting</h1><img src="/asset.svg"></main>
          <script>document.querySelector("#state").textContent = "binding rendered"</script>
          </body></html>`,
        { headers: { "content-type": "text/html; charset=utf-8" } },
      );
    },
  });
  sourceUrl = `http://127.0.0.1:${server.port}/`;
});

afterAll(async () => {
  server.stop(true);
  await rm(directory, { recursive: true, force: true });
});

async function nextEvent(events) {
  return await Promise.race([
    events.next(),
    Bun.sleep(15_000).then(() => {
      throw new Error("timed out waiting for a capture event");
    }),
  ]);
}

test(
  "captures, inspects, emits events, cancels, and closes",
  async () => {
    const output = join(directory, "node-binding.html");
    const cancelledOutput = join(directory, "node-cancelled.html");
    const offprint = new Offprint();

    try {
      await writeFile(output, "stale artifact");
      const result = await offprint.capture(sourceUrl, {
        output,
        selector: "#capture",
        conflict: "replace",
      });

      expect(result.schemaVersion).toBe(2);
      expect(result.artifact.kind).toBe("file");
      expect(result.artifact.path).toBe(output);
      expect(result.verification.networkRequests).toBe(0);
      const content = await readFile(output, "utf8");
      expect(content).toContain("binding rendered");
      expect(content).toContain("data:image/svg+xml;base64,");
      expect(content).not.toContain("outside selector");
      expect(content).not.toContain("stale artifact");

      const manifest = await offprint.artifacts.inspect(output);
      expect(manifest.schemaVersion).toBe(2);
      expect(manifest.source.finalUrl).toBe(sourceUrl);
      expect(manifest.resources.embedded).toBeGreaterThanOrEqual(1);

      const exported = await offprint.artifacts.export(output, {
        schemaVersion: 2,
        outputDirectory: join(directory, "node-formats"),
        baseName: "node-binding",
        conflict: "fail",
        formats: [
          { format: "pdf", options: {} },
          { format: "markdown", options: { frontMatter: true } },
          { format: "zip" },
          { format: "self-extracting-html" },
          { format: "mhtml" },
        ],
      });
      expect(exported.capturePolicySha256).toBe(manifest.capturePolicySha256);
      expect(exported.resources).toEqual(manifest.resources);
      expect(exported.artifacts.map((artifact) => artifact.format)).toEqual([
        "pdf",
        "markdown",
        "zip",
        "self-extracting-html",
        "mhtml",
      ]);
      for (const artifact of exported.artifacts) {
        const verified = await offprint.artifacts.verifyFormat(
          artifact.path,
          artifact.format,
        );
        expect(verified.sha256).toBe(artifact.sha256);
      }

      const captureRequest = offprint.captures.request(sourceUrl, { output: cancelledOutput });
      captureRequest.readiness.delay = 30_000;
      const job = await offprint.captures.start(captureRequest);
      const events = job.events();
      let sawStarted = false;
      while (true) {
        const item = await nextEvent(events);
        expect(item.done).toBe(false);
        sawStarted ||= item.value.type === "capture.started";
        if (item.value.type === "navigation.started") {
          job.cancel();
          break;
        }
      }
      expect(sawStarted).toBe(true);
      await expect(job.result()).rejects.toMatchObject({
        code: "offprint.runtime.cancelled",
        stage: "shutdown",
      });

      let terminal;
      while (true) {
        const item = await nextEvent(events);
        if (item.done) {
          break;
        }
        if (item.value.type === "capture.cancelled") {
          terminal = item.value;
        }
      }
      expect(terminal.captureId).toBe(job.id);
      await expect(stat(cancelledOutput)).rejects.toMatchObject({
        code: "ENOENT",
      });
    } finally {
      await offprint.close();
      await offprint.close();
    }
  },
  60_000,
);

test("validation errors keep the typed binding contract", async () => {
  const offprint = new Offprint();
  try {
    await offprint.capture("javascript:alert(1)");
    throw new Error("capture unexpectedly succeeded");
  } catch (error) {
    expect(error).toBeInstanceOf(OffprintError);
    expect(error).toMatchObject({
      code: "offprint.input.url_scheme",
      stage: "validation",
      retryable: false,
    });
  } finally {
    await offprint.close();
  }
});


test("a configured request captures rendered HTML to bounded memory", async () => {
  const offprint = new Offprint();
  try {
    const request = offprint.captures.request(sourceUrl, { selector: "#capture" });
    request.output = { kind: "memory", maxBytes: 1024 * 1024 };
    const job = await offprint.captures.start(request);
    const receipt = await job.result();

    expect(receipt.artifact.kind).toBe("bytes");
    const html = new TextDecoder().decode(Uint8Array.from(receipt.artifact.content));
    expect(html).toContain("binding rendered");
    expect(receipt.artifact.bytes).toBeLessThanOrEqual(1024 * 1024);
    expect(receipt.verification.mode).toBe("offline");
    expect(receipt.verification.networkRequests).toBe(0);
  } finally {
    await offprint.close();
  }
}, 30_000);
