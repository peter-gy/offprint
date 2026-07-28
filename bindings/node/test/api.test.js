import { afterEach, describe, expect, test } from "bun:test";
import { createRequire } from "node:module";

import { PageKnot, PageKnotError } from "../index.js";

const open = new Set();
const require = createRequire(import.meta.url);

afterEach(async () => {
  await Promise.all([...open].map((pageknot) => pageknot.close()));
  open.clear();
});

function service(options) {
  const pageknot = new PageKnot(options);
  open.add(pageknot);
  return pageknot;
}

describe("PageKnot Node.js binding", () => {
  test("exposes the three canonical services", () => {
    const pageknot = service();

    expect(typeof pageknot.captures.start).toBe("function");
    expect(typeof pageknot.captures.batch).toBe("function");
    expect(typeof pageknot.captures.crawl).toBe("function");
    expect(typeof pageknot.artifacts.inspect).toBe("function");
    expect(typeof pageknot.artifacts.export).toBe("function");
    expect(typeof pageknot.artifacts.verifyVariant).toBe("function");
    expect(typeof pageknot.browsers.ensure).toBe("function");
  });

  test("maps validation failures to structured errors", async () => {
    const pageknot = service();

    try {
      await pageknot.capture("javascript:alert(1)");
      throw new Error("capture unexpectedly succeeded");
    } catch (error) {
      expect(error).toBeInstanceOf(PageKnotError);
      expect(error.code).toBe("pageknot.input.url_scheme");
      expect(error.stage).toBe("validation");
      expect(error.retryable).toBe(false);
      expect(error.details).toEqual({});
    }
  });

  test("rejects misspelled constructor options", () => {
    expect(() => service({ browserPat: "/tmp/chrome" })).toThrow(
      PageKnotError,
    );
  });

  test("close is idempotent", async () => {
    const pageknot = service();

    await pageknot.close();
    await pageknot.close();
    open.delete(pageknot);
  });

  test("the finalizer closes an abandoned native service", async () => {
    const native = require("../native.cjs");
    const originalClose = native.NativePageKnot.prototype.close;
    let closeCalls = 0;
    native.NativePageKnot.prototype.close = function close() {
      closeCalls += 1;
      return originalClose.call(this);
    };
    try {
      let pageknot = new PageKnot();
      const reference = new WeakRef(pageknot);
      pageknot = undefined;
      for (let attempt = 0; attempt < 100 && closeCalls === 0; attempt += 1) {
        Bun.gc(true);
        await Bun.sleep(10);
      }

      expect(reference.deref()).toBeUndefined();
      expect(closeCalls).toBe(1);
    } finally {
      native.NativePageKnot.prototype.close = originalClose;
    }
  });

  test("a retained child service keeps the root service alive", async () => {
    let pageknot = new PageKnot();
    const captures = pageknot.captures;
    const reference = new WeakRef(pageknot);
    pageknot = undefined;

    for (let attempt = 0; attempt < 10; attempt += 1) {
      Bun.gc(true);
      await Bun.sleep(10);
    }

    expect(reference.deref()).toBeDefined();
    expect(typeof captures.start).toBe("function");
    await reference.deref().close();
  });

  test("contains native panics and keeps the host alive", async () => {
    const pageknot = service();

    try {
      await pageknot._testPanic();
      throw new Error("fault injection unexpectedly succeeded");
    } catch (error) {
      expect(error).toBeInstanceOf(PageKnotError);
      expect(error).toMatchObject({
        code: "pageknot.internal.panic",
        stage: "internal",
        retryable: false,
        message: "PageKnot encountered an unexpected internal failure",
      });
      expect(error.message).not.toContain("fault injection");
    }

    await expect(pageknot.capture("javascript:alert(1)")).rejects.toMatchObject({
      code: "pageknot.input.url_scheme",
      stage: "validation",
    });
  });
});
