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

import { PageKnot, PageKnotError } from "../index.js";

const requestFixture = new URL(
  "../../../schemas/examples/capture-request.json",
  import.meta.url,
);

let directory;
let server;
let sourceUrl;

beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), "pageknot-node-"));
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

async function request(output) {
  const value = JSON.parse(await readFile(requestFixture, "utf8"));
  value.url = sourceUrl;
  value.artifact.options.target.value = output;
  return value;
}

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
    const pageknot = new PageKnot();

    try {
      await writeFile(output, "stale artifact");
      const result = await pageknot.capture(sourceUrl, {
        output,
        selector: "#capture",
      });

      expect(result.schemaVersion).toBe(1);
      expect(result.status).toBe("succeeded");
      expect(result.artifact.kind).toBe("file");
      expect(result.artifact.path).toBe(output);
      expect(result.verification.passed).toBe(true);
      expect(result.verification.networkRequests).toBe(0);
      const content = await readFile(output, "utf8");
      expect(content).toContain("binding rendered");
      expect(content).toContain("data:image/svg+xml;base64,");
      expect(content).not.toContain("outside selector");
      expect(content).not.toContain("stale artifact");

      const manifest = await pageknot.artifacts.inspect(output);
      expect(manifest.schemaVersion).toBe(1);
      expect(manifest.source.finalUrl).toBe(sourceUrl);
      expect(manifest.resources.embedded).toBeGreaterThanOrEqual(1);

      const exported = await pageknot.artifacts.export(output, {
        outputDirectory: join(directory, "node-variants"),
        baseName: "node-binding",
        conflict: "fail",
        variants: [
          { kind: "pdf", options: {} },
          { kind: "markdown", options: { frontMatter: true } },
          { kind: "zip" },
          { kind: "self-extracting" },
          { kind: "mhtml" },
        ],
      });
      expect(exported.policySha256).toBe(manifest.policySha256);
      expect(exported.resources).toEqual(manifest.resources);
      expect(exported.variants.map((variant) => variant.kind)).toEqual([
        "pdf",
        "markdown",
        "zip",
        "self-extracting",
        "mhtml",
      ]);
      for (const variant of exported.variants) {
        const verified = await pageknot.artifacts.verifyVariant(
          variant.path,
          variant.kind,
        );
        expect(verified.passed).toBe(true);
        expect(verified.sha256).toBe(variant.sha256);
      }

      const captureRequest = await request(cancelledOutput);
      captureRequest.readiness.delay = 30_000;
      const job = await pageknot.captures.start(captureRequest);
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
        code: "pageknot.runtime.cancelled",
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
      await pageknot.close();
      await pageknot.close();
    }
  },
  60_000,
);

test("validation errors keep the typed binding contract", async () => {
  const pageknot = new PageKnot();
  try {
    await pageknot.capture("javascript:alert(1)");
    throw new Error("capture unexpectedly succeeded");
  } catch (error) {
    expect(error).toBeInstanceOf(PageKnotError);
    expect(error).toMatchObject({
      code: "pageknot.input.url_scheme",
      stage: "validation",
      retryable: false,
    });
  } finally {
    await pageknot.close();
  }
});
