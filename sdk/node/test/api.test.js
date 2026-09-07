import { setTimeout as delay } from "node:timers/promises";
import { afterEach, describe, expect, test } from "vitest";
import { createRequire } from "node:module";

import { Offprint, OffprintError } from "../index.js";

const open = new Set();
const require = createRequire(import.meta.url);

afterEach(async () => {
  await Promise.all([...open].map((offprint) => offprint.close()));
  open.clear();
});

function service(options) {
  const offprint = new Offprint(options);
  open.add(offprint);
  return offprint;
}

describe("Offprint Node.js binding", () => {
  test("exposes the three canonical services", () => {
    const offprint = service();

    expect(typeof offprint.captures.start).toBe("function");
    expect(typeof offprint.captures.batch).toBe("function");
    expect(typeof offprint.captures.crawl).toBe("function");
    expect(typeof offprint.artifacts.inspect).toBe("function");
    expect(typeof offprint.artifacts.export).toBe("function");
    expect(typeof offprint.artifacts.verifyFormat).toBe("function");
    expect(typeof offprint.browsers.ensure).toBe("function");
    expect(typeof offprint.browsers.list).toBe("function");
    expect(typeof offprint.browsers.install).toBe("function");
    expect(typeof offprint.browsers.remove).toBe("function");
    expect(typeof offprint.browsers.doctor).toBe("function");
    expect(typeof offprint.browsers.closeIdle).toBe("function");
    expect(typeof offprint[Symbol.asyncDispose]).toBe("function");
  });

  test("creates independent requests with native defaults and capture options", () => {
    const offprint = service();
    const request = offprint.captures.request("https://example.com", {
      output: "capture.html",
      profile: "server",
      conflict: "replace",
      waitUntil: "network-idle",
      delayMs: 750,
    });

    expect(request.url).toBe("https://example.com/");
    expect(request.output).toEqual({ kind: "file", path: "capture.html", conflict: "replace" });
    expect(request.content.missingResources).toBe("fail");
    expect(request.network).toEqual({ kind: "server" });
    expect(request.readiness.mode).toBe("network-idle");
    expect(request.readiness.delay).toBe(750);
    expect(request.verification).toBe("offline");

    request.environment.viewport.width = 320;
    const next = offprint.captures.request("https://example.com");
    expect(next.environment.viewport.width).toBe(1440);
    expect(next.output).toEqual({ kind: "memory", maxBytes: 64 * 1024 * 1024 });
  });

  test("request edits are validated when the job starts", async () => {
    const offprint = service();
    const request = offprint.captures.request("https://example.com");
    request.limits.duration = 0;

    await expect(offprint.captures.start(request)).rejects.toMatchObject({
      code: "offprint.input.limit",
      stage: "validation",
    });
  });

  test("request construction reports structured synchronous errors", async () => {
    const offprint = service();
    expect(() => offprint.captures.request("javascript:alert(1)")).toThrow(OffprintError);
    expect(() => offprint.captures.request("https://example.com", { timeotMs: 1 })).toThrow(
      OffprintError,
    );
    await offprint.close();
    expect(() => offprint.captures.request("https://example.com")).toThrow(OffprintError);
  });

  test("maps validation failures to structured errors", async () => {
    const offprint = service();

    try {
      await offprint.capture("javascript:alert(1)");
      throw new Error("capture unexpectedly succeeded");
    } catch (error) {
      expect(error).toBeInstanceOf(OffprintError);
      expect(error.code).toBe("offprint.input.url_scheme");
      expect(error.stage).toBe("validation");
      expect(error.retryable).toBe(false);
      expect(error.details).toEqual({});
    }
  });

  test("rejects misspelled constructor options", () => {
    expect(() => service({ browserPat: "/tmp/chrome" })).toThrow(OffprintError);
  });

  test("close is idempotent", async () => {
    const offprint = service();

    await offprint.close();
    await offprint.close();
    open.delete(offprint);
  });

  test("the finalizer closes an abandoned native service", async () => {
    const native = require("../native.cjs");
    const originalClose = native.NativeOffprint.prototype.close;
    let closeCalls = 0;
    native.NativeOffprint.prototype.close = function close() {
      closeCalls += 1;
      return originalClose.call(this);
    };
    try {
      let offprint = new Offprint();
      const reference = new WeakRef(offprint);
      offprint = undefined;
      for (let attempt = 0; attempt < 100 && closeCalls === 0; attempt += 1) {
        global.gc();
        await delay(10);
      }

      expect(reference.deref()).toBeUndefined();
      expect(closeCalls).toBe(1);
    } finally {
      native.NativeOffprint.prototype.close = originalClose;
    }
  });

  test("a retained child service keeps the root service alive", async () => {
    let offprint = new Offprint();
    const captures = offprint.captures;
    const reference = new WeakRef(offprint);
    offprint = undefined;

    for (let attempt = 0; attempt < 10; attempt += 1) {
      global.gc();
      await delay(10);
    }

    expect(reference.deref()).toBeDefined();
    expect(typeof captures.start).toBe("function");
    await reference.deref().close();
  });

  test("contains native panics and keeps the host alive", async () => {
    const offprint = service();

    try {
      await offprint._testPanic();
      throw new Error("fault injection unexpectedly succeeded");
    } catch (error) {
      expect(error).toBeInstanceOf(OffprintError);
      expect(error).toMatchObject({
        code: "offprint.internal.panic",
        stage: "internal",
        retryable: false,
        message: "Offprint encountered an unexpected internal failure",
      });
      expect(error.message).not.toContain("fault injection");
    }

    await expect(offprint.capture("javascript:alert(1)")).rejects.toMatchObject({
      code: "offprint.input.url_scheme",
      stage: "validation",
    });
  });
});
