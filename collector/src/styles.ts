import { cssBaseAttribute } from "./constants";
import {
  adoptedStyleSheets,
  appendChild,
  createElement,
  documentDefaultView,
  isElement,
  localName,
  namespaceUri,
  nodeType,
  ownerDocument,
  querySelector,
  querySelectorAll,
  replaceNode,
  setAttribute,
  setNodeTextContent,
  styleSheets,
} from "./dom";
import {
  arrayJoin,
  arrayPush,
  mapGet,
  mapSet,
  regExpTest,
  SafeSet,
  SafeTypeError,
  setAdd,
  setHas,
  stringMatch,
  stringReplacePattern,
  stringToLocaleLowerCase,
  stringTrim,
} from "./primordials";
import { utf8LengthWithinLimit } from "./protocol";
import type { SnapshotContext } from "./types";
import {
  computedStyle,
  cssRuleAt,
  cssRuleCount,
  cssRuleDeclaration,
  cssRuleSelector,
  cssRuleText,
  cssRuleType,
  rulesForStyleSheet,
  stylePropertyValue,
  styleSheetFor,
  styleSheetHref,
  styleSheetOwner,
  styleSheetsFromList,
} from "./web";

type CopiedCssRules =
  | { attempted: number; kind: "limit" }
  | { bytes: number; kind: "ok"; rules: string[] };

export function copyCssRules(
  sheet: CSSStyleSheet,
  root: Document | ShadowRoot,
  context: SnapshotContext,
  maximumBytes: number,
): CopiedCssRules | undefined {
  try {
    const usedFonts = context.options.removeUnusedFonts
      ? usedFontsForRoot(root, context)
      : undefined;
    const inheritedFontFaces =
      nodeType(root) === 11 && styleSheetOwner(sheet) === null
        ? context.documentFontFaces
        : undefined;
    const copied: string[] = [];
    let bytes = 0;
    const rules = rulesForStyleSheet(sheet);
    const count = cssRuleCount(rules);
    for (let index = 0; index < count; index += 1) {
      if (bytes >= maximumBytes) {
        return { attempted: maximumBytes + 1, kind: "limit" };
      }
      const rule = cssRuleAt(rules, index);
      if (
        rule &&
        keepCssRule(rule, root, context, usedFonts, inheritedFontFaces)
      ) {
        const separatorBytes = copied.length > 0 ? 1 : 0;
        if (bytes > maximumBytes - separatorBytes) {
          return { attempted: maximumBytes + 1, kind: "limit" };
        }
        const remainingBytes = maximumBytes - bytes - separatorBytes;
        if (remainingBytes < 1) {
          return { attempted: maximumBytes + 1, kind: "limit" };
        }
        const text = cssRuleText(rule);
        const width = utf8LengthWithinLimit(text, remainingBytes);
        if (width === null) {
          return { attempted: maximumBytes + 1, kind: "limit" };
        }
        bytes += separatorBytes + width;
        arrayPush(copied, text);
      }
    }
    return { bytes, kind: "ok", rules: copied };
  } catch {
    arrayPush(context.warnings, {
      code: "offprint.cssom.unreadable",
      message: "A stylesheet could not be read through the CSS Object Model.",
    });
    return undefined;
  }
}

export function materializeCssRules(
  sheet: CSSStyleSheet,
  root: Document | ShadowRoot,
  context: SnapshotContext,
  allocatePayload: boolean,
): string | undefined {
  const unclaimedPayloadBytes = context.budget.maximumPayloadAllocation(
    context.reservation,
  );
  const maximumBytes = allocatePayload
    ? unclaimedPayloadBytes
    : context.budget.maximumDocumentBytes(context.reservation);
  // Web CSSOM exposes native rule serialization as one atomic string. Source
  // document bytes are already reserved, so holding every unclaimed byte
  // completes the document allowance before cssText can materialize it.
  context.budget.reservePayloadAllocation(
    context.reservation,
    unclaimedPayloadBytes,
  );
  const copied = copyCssRules(sheet, root, context, maximumBytes);
  if (!copied) {
    context.budget.settlePayloadAllocation(
      context.reservation,
      unclaimedPayloadBytes,
      0,
    );
    return undefined;
  }
  if (copied.kind === "limit") {
    context.budget.settlePayloadAllocation(
      context.reservation,
      unclaimedPayloadBytes,
      0,
    );
    if (allocatePayload) {
      context.budget.rejectPayloadAllocation(
        context.reservation,
        copied.attempted,
      );
    }
    context.budget.rejectPayload(copied.attempted);
  }
  context.budget.settlePayloadAllocation(
    context.reservation,
    unclaimedPayloadBytes,
    allocatePayload ? copied.bytes : 0,
  );
  return arrayJoin(copied.rules, "\n");
}

const statefulSelector =
  /::|:(?:active|any-link|autofill|checked|defined|disabled|enabled|focus|focus-visible|focus-within|fullscreen|future|has|host|hover|indeterminate|link|modal|open|past|paused|picture-in-picture|placeholder-shown|playing|read-only|read-write|required|target|user-invalid|user-valid|valid|visited)\b/i;
const pseudoElements = ["::before", "::after"] as const;

function keepCssRule(
  rule: CSSRule,
  root: Document | ShadowRoot,
  context: SnapshotContext,
  usedFonts: Set<string> | undefined,
  inheritedFontFaces: Set<string> | undefined,
): boolean {
  const kind = cssRuleType(rule);
  if (context.options.removeUnusedCss && kind === 1) {
    const selector = cssRuleSelector(rule as CSSStyleRule);
    if (!regExpTest(statefulSelector, selector)) {
      try {
        if (!querySelector(root, selector)) {
          return false;
        }
      } catch {
        return true;
      }
    }
  }
  // A shadow tree can resolve a global font name from its host tree when it
  // does not define the same face locally.
  if (
    kind === 5 &&
    inheritedFontFaces &&
    setHas(inheritedFontFaces, cssRuleText(rule))
  ) {
    return false;
  }
  if (
    usedFonts &&
    kind === 5 &&
    !hasUsedFont(
      fontFamilies(
        stylePropertyValue(
          cssRuleDeclaration(rule as CSSFontFaceRule),
          "font-family",
        ),
      ),
      usedFonts,
    )
  ) {
    return false;
  }
  return true;
}

function usedFontsForRoot(
  root: Document | ShadowRoot,
  context: SnapshotContext,
): Set<string> {
  const cached = mapGet(context.usedFontsByRoot, root);
  if (cached) {
    return cached;
  }
  const families = usedFontFamilies(root);
  mapSet(context.usedFontsByRoot, root, families);
  return families;
}

function usedFontFamilies(root: Document | ShadowRoot): Set<string> {
  const families = new SafeSet<string>();
  const elements = querySelectorAll(root, "*");
  for (let index = 0; index < elements.length; index += 1) {
    const element = elements[index];
    const document = ownerDocument(element);
    const view = document ? documentDefaultView(document) : null;
    if (!view) {
      continue;
    }
    const style = computedStyle(view, element, null);
    const elementFonts = fontFamilies(stylePropertyValue(style, "font-family"));
    for (
      let familyIndex = 0;
      familyIndex < elementFonts.length;
      familyIndex += 1
    ) {
      setAdd(families, elementFonts[familyIndex]);
    }
    for (
      let pseudoIndex = 0;
      pseudoIndex < pseudoElements.length;
      pseudoIndex += 1
    ) {
      const pseudoStyle = computedStyle(
        view,
        element,
        pseudoElements[pseudoIndex],
      );
      if (stylePropertyValue(pseudoStyle, "content") !== "none") {
        const pseudoFonts = fontFamilies(
          stylePropertyValue(pseudoStyle, "font-family"),
        );
        for (
          let familyIndex = 0;
          familyIndex < pseudoFonts.length;
          familyIndex += 1
        ) {
          setAdd(families, pseudoFonts[familyIndex]);
        }
      }
    }
  }
  return families;
}

function fontFamilies(value: string): string[] {
  const matches = stringMatch(value, /"[^"]*"|'[^']*'|[^,]+/g) ?? [];
  const families: string[] = [];
  for (let index = 0; index < matches.length; index += 1) {
    const family = stringToLocaleLowerCase(
      stringReplacePattern(stringTrim(matches[index]), /^(['"])(.*)\1$/, "$2"),
    );
    if (family) {
      arrayPush(families, family);
    }
  }
  return families;
}

function hasUsedFont(families: string[], usedFonts: Set<string>): boolean {
  for (let index = 0; index < families.length; index += 1) {
    if (setHas(usedFonts, families[index])) {
      return true;
    }
  }
  return false;
}

export function appendAdoptedStyles(
  root: Document | ShadowRoot,
  cloneRoot: Element | DocumentFragment,
  context: SnapshotContext,
): void {
  const sheets = adoptedStyleSheets(root);
  for (let index = 0; index < sheets.length; index += 1) {
    const sheet = sheets[index];
    const css = materializeCssRules(sheet, root, context, true);
    if (css === undefined) {
      continue;
    }
    const style = createElement(documentFor(root), "style");
    setAttribute(style, "data-offprint-adopted", "");
    markStyleBase(style, sheet);
    setNodeTextContent(style, css);
    appendChild(cloneRoot, style);
  }
}

export function applyCssom(
  source: Document | ShadowRoot,
  clones: Map<Node, Node>,
  context: SnapshotContext,
): void {
  const sheets =
    nodeType(source) === 9
      ? styleSheetsFromList(styleSheets(source as Document))
      : ownedStyleSheets(source as ShadowRoot);
  for (let index = 0; index < sheets.length; index += 1) {
    const sheet = sheets[index];
    const owner = styleSheetOwner(sheet);
    if (!owner || nodeType(owner) !== 1) {
      continue;
    }
    const ownerClone = mapGet(clones, owner);
    if (!ownerClone || !isElement(ownerClone)) {
      continue;
    }
    const replacesStyleText =
      namespaceUri(ownerClone) === "http://www.w3.org/1999/xhtml" &&
      localName(ownerClone) === "style";
    const css = materializeCssRules(sheet, source, context, !replacesStyleText);
    if (css === undefined) {
      continue;
    }
    if (replacesStyleText) {
      setNodeTextContent(ownerClone, css);
      markStyleBase(ownerClone, sheet);
    } else {
      const style = createElement(documentFor(source), "style");
      setAttribute(style, "data-offprint-cssom", "");
      markStyleBase(style, sheet);
      setNodeTextContent(style, css);
      replaceNode(ownerClone, style);
    }
  }
}

function markStyleBase(element: Element, sheet: CSSStyleSheet): void {
  const href = styleSheetHref(sheet);
  if (href) {
    setAttribute(element, cssBaseAttribute, href);
  }
}

function ownedStyleSheets(root: ShadowRoot): CSSStyleSheet[] {
  const sheets: CSSStyleSheet[] = [];
  const owners = querySelectorAll(root, 'style, link[rel~="stylesheet"]');
  for (let index = 0; index < owners.length; index += 1) {
    const sheet = styleSheetFor(owners[index]);
    if (sheet) {
      arrayPush(sheets, sheet);
    }
  }
  return sheets;
}

function documentFor(root: Document | ShadowRoot): Document {
  if (nodeType(root) === 9) {
    return root as Document;
  }
  const document = ownerDocument(root);
  if (!document) {
    throw new SafeTypeError("a shadow root has no owner document");
  }
  return document;
}
