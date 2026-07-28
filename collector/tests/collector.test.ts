import { describe, expect, test } from "bun:test";
import ts from "typescript";

if (typeof Element === "undefined") {
  Object.defineProperty(globalThis, "Element", {
    configurable: true,
    value: class {
      get attributes() {
        return Object.getOwnPropertyDescriptor(this, "attributes")?.value;
      }

      get localName() {
        return Object.getOwnPropertyDescriptor(this, "localName")?.value;
      }

      get namespaceURI() {
        return Object.getOwnPropertyDescriptor(this, "namespaceURI")?.value;
      }

      attachShadow() {
        return {};
      }
    },
  });
}

const { sha256Fallback, utf8LengthWithinLimit } = await import("../src/index");
const { countCloneableNodesWithinLimit, RecursiveSnapshotBudget } =
  await import("../src/budget");
const { copyCssRules, frameOwnerMappings, materializeCssRules } =
  await import("../src/collection");
const { serializeJsonBytesBounded } = await import("../src/serialize");
const { maximumPngDataUrlBytes } = await import("../src/state");

interface TestNode {
  nodeType: number;
  firstChild: TestNode | null;
  nextSibling: TestNode | null;
  namespaceURI?: string | null;
  localName?: string | null;
  nodeValue?: string | null;
  content?: TestNode;
}

function node(nodeType = 1, localName?: string): TestNode {
  return {
    nodeType,
    firstChild: null,
    nextSibling: null,
    ...(localName
      ? {
          namespaceURI: "http://www.w3.org/1999/xhtml",
          localName,
        }
      : {}),
  };
}

function append(parent: TestNode, ...children: TestNode[]): void {
  parent.firstChild = children[0] ?? null;
  for (let index = 0; index < children.length; index += 1) {
    children[index].nextSibling = children[index + 1] ?? null;
  }
}

describe("collector source", () => {
  test("matches the shared direct-response wire fixture", async () => {
    const fixture = (await Bun.file(
      new URL(
        "../../crates/pageknot-protocol/fixtures/collector-responses.json",
        import.meta.url,
      ),
    ).json()) as Record<string, unknown>;
    const handshake = globalThis.__pageknotCollector.handshake(
      "cap_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      ["form-state", "frame-owner-mapping"],
      65536,
    );
    const released = globalThis.__pageknotCollector.release(
      "cap_01ARZ3NDEKTSV4RRFFQ69G5FAV",
      7,
    );

    expect(Object.keys(handshake).sort()).toEqual(
      Object.keys(fixture.handshake as object).sort(),
    );
    expect(released).toEqual(
      fixture.released as { captureId: string; frameId: number },
    );
    expect(JSON.parse(JSON.stringify(fixture))).toEqual(fixture);
  });

  test("exposes the versioned protocol and bounded chunk API", async () => {
    const source = await Bun.file(
      new URL("../src/index.ts", import.meta.url),
    ).text();

    const handshake = globalThis.__pageknotCollector.handshake(
      "capture-handshake",
      "host-build",
      [],
      1024,
    );
    expect(handshake.protocol).toEqual({ major: 1, minor: 5 });
    expect(source).toContain("maximumChunkBytes");
    expect(source).toContain("acknowledge");
    expect(source).toContain("release");
  });

  test("measures UTF-8 payloads at the configured boundary", () => {
    expect(utf8LengthWithinLimit("a💾", 5)).toBe(5);
    expect(utf8LengthWithinLimit("a💾", 4)).toBeNull();
    expect(utf8LengthWithinLimit("\ud800", 3)).toBe(3);
    expect(utf8LengthWithinLimit("\ud800", 2)).toBeNull();
  });

  test("returns a structured error for an invalid payload limit", async () => {
    const response = await globalThis.__pageknotCollector.prepare({
      type: "prepare",
      captureScope: "page",
      captureId: "capture-budget",
      frameDepth: 0,
      frameId: 1,
      maximumChunkBytes: 1024,
      maximumFrameDepth: 8,
      maximumFrames: 8,
      maximumNodes: 100,
      maximumPayloadBytes: 0,
      preservePasswordValues: false,
      selector: undefined,
      removeHiddenElements: false,
      removeUnusedCss: false,
      removeUnusedFonts: false,
    });

    expect(response).toEqual({
      type: "error",
      payload: {
        captureId: "capture-budget",
        code: "pageknot.collector.payload_limit",
        message: "collector payload exceeds the configured observation limit",
        details: {
          limit: 0,
        },
      },
    });
  });

  test("counts document types, text, comments, templates, and observed shadow descendants", () => {
    const document = node(9);
    const doctype = node(10);
    const html = node(1, "html");
    const head = node(1, "head");
    const titleText = node(3);
    const body = node(1, "body");
    const openHost = node(1, "section");
    const closedHost = node(1, "article");
    const template = node(1, "template");
    const comment = node(8);
    const openChild = node(1, "strong");
    const openText = node(3);
    const closedComment = node(8);
    const templateChild = node(1, "span");
    const templateText = node(3);

    append(document, doctype, html);
    append(html, head, body);
    append(head, titleText);
    append(body, openHost, closedHost, template, comment);
    append(openChild, openText);
    append(templateChild, templateText);
    const templateContent = node(11);
    const openShadow = node(11);
    const closedShadow = node(11);
    append(templateContent, templateChild);
    append(openShadow, openChild);
    append(closedShadow, closedComment);
    template.content = templateContent;
    const shadows = new Map<TestNode, TestNode>([
      [openHost, openShadow],
      [closedHost, closedShadow],
    ]);
    const shadowFor = (element: TestNode) => shadows.get(element);

    expect(countCloneableNodesWithinLimit(document, 14, shadowFor)).toBe(14);
  });

  test("accepts the exact node boundary and stops at limit plus one", () => {
    const document = node(9);
    const html = node(1, "html");
    const text = node(3);
    const comment = node(8);
    append(document, html);
    append(html, text, comment);
    const noShadow = () => undefined;

    expect(countCloneableNodesWithinLimit(document, 3, noShadow)).toBe(3);
    expect(countCloneableNodesWithinLimit(document, 2, noShadow)).toBe(3);
    expect(countCloneableNodesWithinLimit(document, 1, noShadow)).toBe(2);
  });

  test("shares frame, depth, node, and source payload reservations", () => {
    const source = node(9);
    const text = node(3);
    text.nodeValue = "frame";
    append(source, text);

    const frameBudget = new RecursiveSnapshotBudget(
      "capture-frames",
      10,
      100,
      1,
      8,
    );
    expect(
      frameBudget.reserveDocument(source as unknown as Document, 0),
    ).toEqual({
      allocatedPayloadBytes: 0,
      nestedFrames: 0,
      nestedNodes: 0,
      nestedPayloadBytes: 0,
      nodes: 1,
      sourcePayloadBytes: 5,
    });
    try {
      frameBudget.reserveDocument(source as unknown as Document, 1);
      throw new Error("expected the frame reservation to fail");
    } catch (error) {
      expect(frameBudget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.frame.limit",
        details: { attempted: 2, limit: 1 },
      });
    }

    const depthBudget = new RecursiveSnapshotBudget(
      "capture-depth",
      10,
      100,
      2,
      0,
    );
    try {
      depthBudget.reserveDocument(source as unknown as Document, 1);
      throw new Error("expected the depth reservation to fail");
    } catch (error) {
      expect(depthBudget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.frame.depth",
        details: { attempted: 1, limit: 0 },
      });
    }

    const payloadBudget = new RecursiveSnapshotBudget(
      "capture-payload",
      10,
      4,
      2,
      8,
    );
    try {
      payloadBudget.reserveDocument(source as unknown as Document, 0);
      throw new Error("expected the payload reservation to fail");
    } catch (error) {
      expect(payloadBudget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.collector.payload_limit",
        details: { attempted: 5, limit: 4 },
      });
    }
  });

  test("streams attribute preflight and stops at the payload boundary", () => {
    let attributeValuesRead = 0;
    const document = node(9);
    const element = node(1, "main") as TestNode & {
      attributes: {
        length: number;
        item(index: number): { name: string; value: string } | null;
      };
    };
    const attributeNodes = [
      {
        name: "first",
        get value() {
          attributeValuesRead += 1;
          return "x".repeat(64);
        },
      },
      {
        name: "late",
        get value() {
          attributeValuesRead += 1;
          return "must not be read";
        },
      },
    ];
    element.attributes = {
      length: attributeNodes.length,
      item(index) {
        return attributeNodes[index] ?? null;
      },
    };
    append(document, element);
    const budget = new RecursiveSnapshotBudget(
      "capture-attributes",
      10,
      16,
      1,
      0,
    );

    try {
      budget.reserveDocument(document as unknown as Document, 0);
      throw new Error("expected attribute preflight to exceed the payload");
    } catch (error) {
      expect(budget.limitResponse(error)?.payload.code).toBe(
        "pageknot.collector.payload_limit",
      );
    }
    expect(attributeValuesRead).toBe(1);
  });

  test("holds bounded payload allocations until document commit", () => {
    const source = node(9);
    const text = node(3);
    text.nodeValue = "frame";
    append(source, text);
    const budget = new RecursiveSnapshotBudget(
      "capture-allocation",
      10,
      32,
      1,
      0,
    );
    const reservation = budget.reserveDocument(
      source as unknown as Document,
      0,
    );

    budget.reservePayloadAllocation(reservation, 20);
    expect(budget.maximumPayloadAllocation(reservation)).toBe(7);
    budget.settlePayloadAllocation(reservation, 20, 8);
    expect(budget.maximumPayloadAllocation(reservation)).toBe(19);
    try {
      budget.reservePayloadAllocation(reservation, 20);
      throw new Error("expected the held allocation to enforce the payload");
    } catch (error) {
      expect(budget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.collector.payload_limit",
        details: { limit: 32 },
      });
    }
    budget.commitDocument(reservation, 13);
  });

  test("streams CSSRuleList items until the payload boundary", () => {
    let itemsRead = 0;
    let ruleTextsRead = 0;
    const list = {
      length: 100_000,
      item() {
        itemsRead += 1;
        return {
          type: 1,
          get cssText() {
            ruleTextsRead += 1;
            return "a";
          },
        };
      },
    };
    const copied = copyCssRules(
      { cssRules: list } as unknown as CSSStyleSheet,
      {} as Document,
      {
        options: {
          removeUnusedCss: false,
          removeUnusedFonts: false,
        },
        warnings: [],
      } as unknown as Parameters<typeof copyCssRules>[2],
      7,
    );

    expect(copied).toEqual({
      attempted: 8,
      kind: "limit",
    });
    expect(itemsRead).toBe(4);
    expect(ruleTextsRead).toBe(4);
  });

  test("omits adopted font faces already defined by the document", () => {
    const fontFace =
      '@font-face { font-family: "Fixture"; src: url("./fixture.woff2"); }';
    const copied = copyCssRules(
      {
        ownerNode: null,
        cssRules: {
          length: 2,
          item(index: number) {
            return index === 0
              ? { type: 5, cssText: fontFace }
              : { type: 1, cssText: ".label { font-family: Fixture; }" };
          },
        },
      } as unknown as CSSStyleSheet,
      { nodeType: 11 } as unknown as ShadowRoot,
      {
        documentFontFaces: new Set([fontFace]),
        options: {
          removeUnusedCss: false,
          removeUnusedFonts: false,
        },
        warnings: [],
      } as unknown as Parameters<typeof copyCssRules>[2],
      1024,
    );

    expect(copied).toEqual({
      bytes: 32,
      kind: "ok",
      rules: [".label { font-family: Fixture; }"],
    });
  });

  test("reserves the remaining payload before native CSS rule serialization", () => {
    const source = node(9);
    const budget = new RecursiveSnapshotBudget("capture-css-rule", 10, 4, 1, 0);
    const reservation = budget.reserveDocument(
      source as unknown as Document,
      0,
    );
    let sawReservation = false;
    const sheet = {
      cssRules: {
        length: 1,
        item() {
          return {
            type: 1,
            get cssText() {
              sawReservation = reservation.allocatedPayloadBytes === 4;
              return "x".repeat(64);
            },
          };
        },
      },
    };

    try {
      materializeCssRules(
        sheet as unknown as CSSStyleSheet,
        {} as Document,
        {
          budget,
          options: {
            removeUnusedCss: false,
            removeUnusedFonts: false,
          },
          reservation,
          warnings: [],
        } as unknown as Parameters<typeof materializeCssRules>[2],
        true,
      );
      throw new Error("expected CSS rule serialization to exceed the payload");
    } catch (error) {
      expect(budget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.collector.payload_limit",
        details: { attempted: 5, limit: 4 },
      });
    }
    expect(sawReservation).toBe(true);
    expect(reservation.allocatedPayloadBytes).toBe(0);
    expect(budget.maximumPayloadAllocation(reservation)).toBe(4);
  });

  test("completes the document allowance before replacement CSS serialization", () => {
    const source = node(9);
    const text = node(3);
    text.nodeValue = "base";
    append(source, text);
    const budget = new RecursiveSnapshotBudget(
      "capture-style-replacement",
      10,
      8,
      1,
      0,
    );
    const reservation = budget.reserveDocument(
      source as unknown as Document,
      0,
    );
    let sawCompleteAllowance = false;
    const sheet = {
      cssRules: {
        length: 1,
        item() {
          return {
            type: 1,
            get cssText() {
              sawCompleteAllowance =
                reservation.sourcePayloadBytes === 4 &&
                reservation.allocatedPayloadBytes === 4 &&
                budget.maximumDocumentBytes(reservation) === 8;
              return "x".repeat(64);
            },
          };
        },
      },
    };

    try {
      materializeCssRules(
        sheet as unknown as CSSStyleSheet,
        {} as Document,
        {
          budget,
          options: {
            removeUnusedCss: false,
            removeUnusedFonts: false,
          },
          reservation,
          warnings: [],
        } as unknown as Parameters<typeof materializeCssRules>[2],
        false,
      );
      throw new Error("expected replacement CSS to exceed the document");
    } catch (error) {
      expect(budget.limitResponse(error)?.payload).toMatchObject({
        code: "pageknot.collector.payload_limit",
        details: { attempted: 9, limit: 8 },
      });
    }
    expect(sawCompleteAllowance).toBe(true);
    expect(reservation.allocatedPayloadBytes).toBe(0);
    expect(budget.maximumPayloadAllocation(reservation)).toBe(4);
  });

  test("bounds canvas encoding before PNG data URL materialization", () => {
    expect(maximumPngDataUrlBytes(1, 1)).toBeGreaterThan(22);
    expect(maximumPngDataUrlBytes(4096, 4096)).toBeGreaterThan(
      64 * 1024 * 1024,
    );
    expect(maximumPngDataUrlBytes(Number.MAX_SAFE_INTEGER, 2)).toBeNull();
  });

  test("prefixes nested frame-owner paths through inline documents", () => {
    expect(
      frameOwnerMappings(2, 1, [
        { originalPath: [0], retainedPath: [0] },
        { originalPath: [1, 0], retainedPath: [1, 0] },
      ]),
    ).toEqual([
      { originalPath: [2], retainedPath: [1] },
      { originalPath: [2, 0], retainedPath: [1, 0] },
      { originalPath: [2, 1, 0], retainedPath: [1, 1, 0] },
    ]);
  });

  test("serializes JSON into an exact bounded UTF-8 allocation", () => {
    const value = {
      escaped: "<line>\n",
      nested: [true, null, "💾"],
      finite: 4,
    };
    const expected = JSON.stringify(value);
    const expectedBytes = new TextEncoder().encode(expected);
    const originalStringify = JSON.stringify;
    const originalTextEncoder = globalThis.TextEncoder;
    try {
      JSON.stringify = () => {
        throw new Error("page JSON.stringify must not run");
      };
      Object.defineProperty(globalThis, "TextEncoder", {
        configurable: true,
        value: class {
          constructor() {
            throw new Error("page TextEncoder must not run");
          }
        },
      });
      const exact = serializeJsonBytesBounded(value, expectedBytes.byteLength);
      expect(exact).toEqual({
        bytes: expectedBytes.byteLength,
        kind: "ok",
        value: expectedBytes,
      });
      expect(
        serializeJsonBytesBounded(value, expectedBytes.byteLength - 1),
      ).toEqual({
        attempted: expectedBytes.byteLength,
        kind: "limit",
      });
    } finally {
      JSON.stringify = originalStringify;
      Object.defineProperty(globalThis, "TextEncoder", {
        configurable: true,
        value: originalTextEncoder,
      });
    }
  });

  test("hashes payloads in contexts without Web Crypto", () => {
    expect(sha256Fallback(new Uint8Array())).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
    expect(sha256Fallback(new TextEncoder().encode("abc"))).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
    for (const length of [1, 55, 56, 63, 64, 65, 1024]) {
      const bytes = Uint8Array.from(
        { length },
        (_, index) => (index * 31 + 17) & 0xff,
      );
      const expected = new Bun.CryptoHasher("sha256")
        .update(bytes)
        .digest("hex");
      expect(sha256Fallback(bytes)).toBe(expected);
    }
  });

  test("keeps production modules focused and the entrypoint compositional", async () => {
    const files = [
      "collection.ts",
      "budget.ts",
      "constants.ts",
      "dispatch.ts",
      "dom.ts",
      "index.ts",
      "motion.ts",
      "primordials.ts",
      "protocol.ts",
      "serialize.ts",
      "shadow.ts",
      "scope.ts",
      "state.ts",
      "types.ts",
      "web.ts",
    ];
    for (const file of files) {
      const source = await Bun.file(
        new URL(`../src/${file}`, import.meta.url),
      ).text();
      expect(source.split("\n").length).toBeLessThan(1000);
    }
    const index = await Bun.file(
      new URL("../src/index.ts", import.meta.url),
    ).text();
    expect(index.split("\n").length).toBeLessThan(300);
  });

  test("avoids page-controlled iteration, setters, and dispatch", async () => {
    const files = [
      "budget.ts",
      "collection.ts",
      "dispatch.ts",
      "index.ts",
      "motion.ts",
      "protocol.ts",
      "serialize.ts",
      "shadow.ts",
      "state.ts",
    ];
    const mutableMethods = new Set([
      "append",
      "appendChild",
      "cloneNode",
      "createElement",
      "filter",
      "forEach",
      "getAnimations",
      "getAttribute",
      "getBoundingClientRect",
      "getComputedStyle",
      "getKeyframes",
      "getPropertyValue",
      "hasAttribute",
      "includes",
      "insertBefore",
      "join",
      "map",
      "match",
      "pause",
      "pop",
      "push",
      "querySelector",
      "querySelectorAll",
      "removeAttribute",
      "removeChild",
      "replace",
      "replaceChild",
      "setAttribute",
      "setProperty",
      "some",
      "startsWith",
      "test",
      "toLocaleLowerCase",
      "toLowerCase",
      "toggleAttribute",
      "trim",
    ]);
    const failures: string[] = [];

    for (const file of files) {
      const sourceText = await Bun.file(
        new URL(`../src/${file}`, import.meta.url),
      ).text();
      const source = ts.createSourceFile(
        file,
        sourceText,
        ts.ScriptTarget.Latest,
        true,
        ts.ScriptKind.TS,
      );
      const visit = (node: ts.Node): void => {
        const unsupportedSpread =
          ts.isSpreadElement(node) &&
          (ts.isArrayLiteralExpression(node.parent) ||
            ts.isCallExpression(node.parent) ||
            ts.isNewExpression(node.parent));
        const mutableDispatch =
          ts.isCallExpression(node) &&
          ts.isPropertyAccessExpression(node.expression) &&
          mutableMethods.has(node.expression.name.text);
        const mutableGlobal =
          ts.isPropertyAccessExpression(node) &&
          ((node.expression.getText(source) === "JSON" &&
            node.name.text === "stringify") ||
            (node.expression.getText(source) === "Reflect" &&
              node.name.text === "apply") ||
            (node.expression.getText(source) === "Array" &&
              node.name.text === "from"));
        const mutableTimer =
          ts.isCallExpression(node) &&
          ts.isIdentifier(node.expression) &&
          ["requestAnimationFrame", "setTimeout"].includes(
            node.expression.text,
          );
        const mutableSetter =
          ts.isBinaryExpression(node) &&
          node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
          ts.isPropertyAccessExpression(node.left) &&
          node.left.name.text === "type";
        const iterableSafeCollectionConstruction =
          ts.isNewExpression(node) &&
          ["SafeMap", "SafeSet", "SafeWeakMap", "SafeWeakSet"].includes(
            node.expression.getText(source),
          ) &&
          (node.arguments?.length ?? 0) > 0;
        if (
          ts.isForOfStatement(node) ||
          ts.isArrayBindingPattern(node) ||
          unsupportedSpread ||
          iterableSafeCollectionConstruction ||
          mutableDispatch ||
          mutableGlobal ||
          mutableTimer ||
          mutableSetter ||
          (ts.isBinaryExpression(node) &&
            node.operatorToken.kind === ts.SyntaxKind.InstanceOfKeyword) ||
          (ts.isNewExpression(node) &&
            ["Promise", "TextEncoder"].includes(
              node.expression.getText(source),
            ))
        ) {
          const position = source.getLineAndCharacterOfPosition(
            node.getStart(),
          );
          failures.push(`${file}:${position.line + 1}`);
        }
        ts.forEachChild(node, visit);
      };
      visit(source);
    }

    expect(failures).toEqual([]);
  });

  test("preserves generated motion metadata during reserved cleanup", async () => {
    const motionText = await Bun.file(
      new URL("../src/motion.ts", import.meta.url),
    ).text();
    const motion = ts.createSourceFile(
      "motion.ts",
      motionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const appendMotionStyles = motion.statements.find(
      (statement): statement is ts.FunctionDeclaration =>
        ts.isFunctionDeclaration(statement) &&
        statement.name?.text === "appendMotionStyles",
    );
    let registration: ts.CallExpression | undefined;
    let insertion: ts.CallExpression | undefined;
    const visitMotion = (node: ts.Node): void => {
      if (ts.isCallExpression(node)) {
        const expression = node.expression.getText(motion);
        const arguments_ = node.arguments.map((argument) =>
          argument.getText(motion),
        );
        if (
          expression === "weakSetAdd" &&
          arguments_[0] === "generatedMotionStyles" &&
          arguments_[1] === "style"
        ) {
          registration = node;
        }
        if (
          expression === "appendChild" &&
          arguments_[0] === "root" &&
          arguments_[1] === "style"
        ) {
          insertion = node;
        }
      }
      ts.forEachChild(node, visitMotion);
    };
    if (appendMotionStyles) {
      visitMotion(appendMotionStyles);
    }

    const collectionText = await Bun.file(
      new URL("../src/collection.ts", import.meta.url),
    ).text();
    const collection = ts.createSourceFile(
      "collection.ts",
      collectionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const cleanup = collection.statements.find(
      (statement): statement is ts.FunctionDeclaration =>
        ts.isFunctionDeclaration(statement) &&
        statement.name?.text === "removeReservedMetadata",
    );
    let preservesGenerated = false;
    const visitCleanup = (node: ts.Node): void => {
      if (
        ts.isPrefixUnaryExpression(node) &&
        node.operator === ts.SyntaxKind.ExclamationToken &&
        ts.isCallExpression(node.operand) &&
        node.operand.expression.getText(collection) === "isGeneratedMotionStyle"
      ) {
        preservesGenerated = true;
      }
      ts.forEachChild(node, visitCleanup);
    };
    if (cleanup) {
      visitCleanup(cleanup);
    }

    expect(registration).toBeDefined();
    expect(insertion).toBeDefined();
    expect(registration?.getStart()).toBeLessThan(insertion?.getStart() ?? 0);
    expect(preservesGenerated).toBe(true);
  });

  test("snapshots the shadow mode before native attachment", async () => {
    const sourceText = await Bun.file(
      new URL("../src/shadow.ts", import.meta.url),
    ).text();
    const source = ts.createSourceFile(
      "shadow.ts",
      sourceText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const modeReads: ts.PropertyAccessExpression[] = [];
    const nativeCalls: ts.CallExpression[] = [];
    const visit = (node: ts.Node): void => {
      if (
        ts.isPropertyAccessExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "init" &&
        node.name.text === "mode"
      ) {
        modeReads.push(node);
      }
      if (
        ts.isCallExpression(node) &&
        ts.isIdentifier(node.expression) &&
        node.expression.text === "safeReflectApply" &&
        node.arguments[0]?.getText(source) === "originalAttachShadow"
      ) {
        nativeCalls.push(node);
      }
      ts.forEachChild(node, visit);
    };
    visit(source);

    expect(modeReads).toHaveLength(1);
    expect(nativeCalls).toHaveLength(1);
    expect(modeReads[0]?.getStart()).toBeLessThan(
      nativeCalls[0]?.getStart() ?? 0,
    );
    const forwarded = nativeCalls[0]?.arguments[2];
    const forwardsPageObject =
      forwarded !== undefined &&
      ts.isArrayLiteralExpression(forwarded) &&
      forwarded.elements.some(
        (element) => ts.isIdentifier(element) && element.text === "init",
      );
    expect(forwardsPageObject).toBe(false);
  });
});
