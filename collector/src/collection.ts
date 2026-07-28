import {
  animationStyleAttribute,
  cssBaseAttribute,
  documentScrollXAttribute,
  documentScrollYAttribute,
  manifestElementId,
  repairDataElementId,
  repairMarkerAttribute,
  repairMediaType,
  repairScriptElementId,
  stateScriptElementId,
} from "./constants";
import {
  adoptedStyleSheets,
  appendChild,
  childNodes,
  cloneNode,
  contains,
  createDocumentFragment,
  createElement,
  createTreeWalker,
  capturedTreeWalkerNodes,
  doctypeName,
  doctypePublicId,
  doctypeSystemId,
  documentBody,
  documentDefaultView,
  documentDoctype,
  documentElement,
  elementChildren,
  getAttribute,
  getSelection,
  hasAttribute,
  isElement,
  localName,
  namespaceUri,
  nodeType,
  nodeValue,
  ownerDocument,
  parentNode,
  parseHtml,
  querySelector,
  querySelectorAll,
  removeAttribute,
  removeChild,
  removeNode,
  replaceNode,
  setAttribute,
  setNodeTextContent,
  styleSheets,
  templateContent,
} from "./dom";
import {
  appendMotionStyles,
  freezeMotion,
  isGeneratedMotionStyle,
  materializeMotionState,
  reportMotionCaptureFailure,
} from "./motion";
import { createInspectableShadowTemplate, observedShadowRoot } from "./shadow";
import {
  arrayIncludes,
  arrayJoin,
  arrayPop,
  arrayPush,
  mapGet,
  mapSet,
  SafeMap,
  SafeSet,
  SafeString,
  SafeTypeError,
  regExpTest,
  setAdd,
  setHas,
  stringMatch,
  stringReplacePattern,
  stringToLocaleLowerCase,
  stringTrim,
} from "./primordials";
import {
  escapeScriptDataBounded,
  serializeHtmlBounded,
  serializeJsonStringBounded,
} from "./serialize";
import { applySelectorScope } from "./scope";
import { utf8LengthWithinLimit } from "./protocol";
import {
  copyState,
  prepareElementState,
  type InlineSnapshotter,
} from "./state";
import type {
  FrameOwnerSnapshot,
  InlineSnapshot,
  RepairNode,
  SelectionSnapshot,
  SnapshotContext,
  SnapshotRootContext,
  InlineSnapshotRequest,
  SnapshotWarning,
  StructuralRepairTree,
  VisualFallback,
} from "./types";
import {
  computedStyle,
  cssRuleAt,
  cssRuleCount,
  cssRuleDeclaration,
  cssRuleSelector,
  cssRuleText,
  cssRuleType,
  documentFontFaces,
  rangeCollapsed,
  rangeIntersectsNode,
  rulesForStyleSheet,
  selectionRange,
  selectionRangeCount,
  stylePropertyValue,
  styleSheetFor,
  styleSheetHref,
  styleSheetOwner,
  styleSheetsFromList,
  treeWalkerCurrent,
  treeWalkerNext,
  windowScrollX,
  windowScrollY,
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
      ? usedFontFamilies(root)
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
      code: "pageknot.cssom.unreadable",
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
    const pseudos = ["::before", "::after"];
    for (let pseudoIndex = 0; pseudoIndex < pseudos.length; pseudoIndex += 1) {
      const pseudoStyle = computedStyle(view, element, pseudos[pseudoIndex]);
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

function appendAdoptedStyles(
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
    setAttribute(style, "data-pageknot-adopted", "");
    markStyleBase(style, sheet);
    setNodeTextContent(style, css);
    appendChild(cloneRoot, style);
  }
}

function copyShadowRoot(
  liveRoot: ShadowRoot,
  cloneHost: Element,
  context: SnapshotContext,
): void {
  const template = createInspectableShadowTemplate(liveRoot);
  const document = ownerDocument(liveRoot);
  if (!document) {
    throw new SafeTypeError("a shadow root has no owner document");
  }
  const fragment = createDocumentFragment(document);
  const liveChildren = childNodes(liveRoot);
  for (let index = 0; index < liveChildren.length; index += 1) {
    appendChild(fragment, cloneNode(liveChildren[index], true));
  }
  const motionRules: string[] = [];
  const clones = materializePairs(liveRoot, fragment, context, motionRules);
  applyCssom(liveRoot, clones, context);
  appendAdoptedStyles(liveRoot, fragment, context);
  appendMotionStyles(fragment, motionRules);
  appendChild(templateContent(template), fragment);
  appendChild(cloneHost, template);
}

function materializePairs(
  liveRoot: Node,
  cloneRoot: Node,
  context: SnapshotContext,
  motionRules: string[],
): Map<Node, Node> {
  const stack: Array<[Node, Node]> = [[liveRoot, cloneRoot]];
  const pairs: Array<[Node, Node]> = [];
  const clones = new SafeMap<Node, Node>();
  while (stack.length > 0) {
    const pair = arrayPop(stack);
    if (!pair) {
      break;
    }
    arrayPush(pairs, pair);
    const live = pair[0];
    const clone = pair[1];
    const liveChildren = childNodes(live);
    const cloneChildren = childNodes(clone);
    const count =
      liveChildren.length < cloneChildren.length
        ? liveChildren.length
        : cloneChildren.length;
    for (let index = count - 1; index >= 0; index -= 1) {
      arrayPush(stack, [liveChildren[index], cloneChildren[index]]);
    }
  }

  const snapshotInline: InlineSnapshotter = snapshotInlineDocument;
  for (let pairIndex = 0; pairIndex < pairs.length; pairIndex += 1) {
    const live = pairs[pairIndex][0];
    const clone = pairs[pairIndex][1];
    mapSet(clones, live, clone);
    if (isElement(live) && isElement(clone)) {
      removeAttribute(clone, cssBaseAttribute);
      prepareElementState(live, clone);
      materializeMotionState(live, clone, context, motionRules);
    }
    copyState(live, clone, context, snapshotInline);
    if (isElement(live) && isElement(clone)) {
      const liveElement = live as Element;
      const shadow = observedShadowRoot(liveElement);
      if (shadow) {
        copyShadowRoot(shadow, clone as Element, context);
      }
      if (
        context.options.removeHiddenElements &&
        isLayoutlessHiddenElement(liveElement)
      ) {
        removeNode(clone);
      }
    }
  }
  return clones;
}

function applyCssom(
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
      setAttribute(style, "data-pageknot-cssom", "");
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

function isLayoutlessHiddenElement(element: Element): boolean {
  const document = ownerDocument(element);
  const view = document ? documentDefaultView(document) : null;
  const body = document ? documentBody(document) : null;
  return (
    (body ? contains(body, element) : false) &&
    (view
      ? stylePropertyValue(computedStyle(view, element, null), "display")
      : "") === "none"
  );
}

function applySelectionScope(
  source: Document,
  clones: Map<Node, Node>,
): SelectionSnapshot {
  const selection = getSelection(source);
  const ranges: Range[] = [];
  if (selection) {
    const rangeCount = selectionRangeCount(selection);
    for (let index = 0; index < rangeCount; index += 1) {
      const range = selectionRange(selection, index);
      if (!rangeCollapsed(range)) {
        arrayPush(ranges, range);
      }
    }
  }
  const body = documentBody(source);
  if (!body || ranges.length === 0) {
    return { ranges: 0, nodes: 0 };
  }

  const selected = new SafeSet<Node>();
  setAdd(selected, body);
  const nodes: Node[] = [];
  const walker = createTreeWalker(source, body, capturedTreeWalkerNodes);
  while (treeWalkerNext(walker)) {
    arrayPush(nodes, treeWalkerCurrent(walker));
  }
  for (let rangeIndex = 0; rangeIndex < ranges.length; rangeIndex += 1) {
    const range = ranges[rangeIndex];
    for (let nodeIndex = 0; nodeIndex < nodes.length; nodeIndex += 1) {
      const node = nodes[nodeIndex];
      try {
        if (!rangeIntersectsNode(range, node)) {
          continue;
        }
      } catch {
        continue;
      }
      for (
        let current: Node | null = node;
        current && current !== body;
        current = parentNode(current)
      ) {
        setAdd(selected, current);
      }
    }
  }

  for (let index = 0; index < nodes.length; index += 1) {
    const node = nodes[index];
    if (
      !setHas(selected, node) &&
      parentNode(node) &&
      setHas(selected, parentNode(node) as Node)
    ) {
      const clone = mapGet(clones, node);
      if (clone) {
        removeNode(clone);
      }
    }
  }
  const cloneBody = mapGet(clones, body);
  if (cloneBody && isElement(cloneBody)) {
    setAttribute(cloneBody, "data-pageknot-selection", "");
  }
  return {
    ranges: ranges.length,
    nodes:
      1 +
      (cloneBody && isElement(cloneBody)
        ? querySelectorAll(cloneBody, "*").length
        : 0),
  };
}

export function snapshotDocument(
  source: Document,
  rootContext: SnapshotRootContext,
): {
  frames: number;
  html: string;
  nodes: number;
  payloadBytes: number;
  selection: SelectionSnapshot;
  subtreeNodes: number;
  frameOwners: FrameOwnerSnapshot[];
} {
  const reservation = rootContext.budget.reserveDocument(
    source,
    rootContext.frameDepth,
  );
  const context: SnapshotContext = {
    ...rootContext,
    documentFontFaces: documentFontFaces(source),
    inlineFrameOwners: new SafeMap<Element, FrameOwnerSnapshot[]>(),
    reservation,
  };
  freezeMotion(source);
  reportMotionCaptureFailure(source, context);
  const liveDocumentElement = documentElement(source);
  const clone = cloneNode(liveDocumentElement, true) as Element;
  const motionRules: string[] = [];
  const clones = materializePairs(
    liveDocumentElement,
    clone,
    context,
    motionRules,
  );
  const view = documentDefaultView(source);
  setAttribute(
    clone,
    documentScrollXAttribute,
    SafeString(view ? windowScrollX(view) : 0),
  );
  setAttribute(
    clone,
    documentScrollYAttribute,
    SafeString(view ? windowScrollY(view) : 0),
  );
  const liveFrameOwners = querySelectorAll(source, "iframe, frame");
  const removableSources = querySelectorAll(
    clone,
    "picture source, video source, audio source",
  );
  for (let index = 0; index < removableSources.length; index += 1) {
    removeNode(removableSources[index]);
  }
  applyCssom(source, clones, context);
  const head = querySelector(clone, "head");
  if (head) {
    appendMotionStyles(head, motionRules);
  }
  const selection =
    context.options.captureScope === "selection"
      ? applySelectionScope(source, clones)
      : { ranges: 0, nodes: 0 };
  if (rootContext.selectorTarget) {
    applySelectorScope(source, rootContext.selectorTarget, clone, clones);
  }
  const adoptedStyleHost = querySelector(clone, "body") ?? head;
  if (adoptedStyleHost) {
    appendAdoptedStyles(source, adoptedStyleHost, context);
  }
  removeReservedMetadata(clone);
  const frameOwners: FrameOwnerSnapshot[] = [];
  let retainedIndex = 0;
  for (
    let originalIndex = 0;
    originalIndex < liveFrameOwners.length;
    originalIndex += 1
  ) {
    const liveOwner = liveFrameOwners[originalIndex];
    const cloneOwner = mapGet(clones, liveOwner);
    if (cloneOwner && isElement(cloneOwner) && contains(clone, cloneOwner)) {
      const mappings = frameOwnerMappings(
        originalIndex,
        retainedIndex,
        mapGet(context.inlineFrameOwners, cloneOwner) ?? [],
      );
      for (let index = 0; index < mappings.length; index += 1) {
        arrayPush(frameOwners, mappings[index]);
      }
      retainedIndex += 1;
    }
  }
  assignRepairMarkers(clone);
  const maximumDocumentBytes = context.budget.maximumDocumentBytes(reservation);
  let serialized = serializeHtmlBounded(clone, maximumDocumentBytes);
  if (serialized.kind === "limit") {
    context.budget.rejectPayload(serialized.attempted);
  }
  const structuralRepair = structuralRepairFor(clone, serialized.value);
  if (structuralRepair) {
    const repairJson = serializeJsonStringBounded(
      structuralRepair,
      maximumDocumentBytes,
    );
    if (repairJson.kind === "limit") {
      context.budget.rejectPayload(repairJson.attempted);
    }
    const repairData = escapeScriptDataBounded(
      repairJson.value,
      maximumDocumentBytes,
    );
    if (repairData.kind === "limit") {
      context.budget.rejectPayload(repairData.attempted);
    }
    appendRepairData(source, clone, repairData.value);
  } else {
    removeRepairMarkers(clone);
  }
  serialized = serializeHtmlBounded(clone, maximumDocumentBytes);
  if (serialized.kind === "limit") {
    context.budget.rejectPayload(serialized.attempted);
  }
  context.budget.commitDocument(reservation, serialized.bytes);
  return {
    frames: reservation.nestedFrames + 1,
    html: serialized.value,
    nodes: reservation.nodes,
    payloadBytes: serialized.bytes,
    selection,
    subtreeNodes: reservation.nodes + reservation.nestedNodes,
    frameOwners,
  };
}

export function frameOwnerMappings(
  originalIndex: number,
  retainedIndex: number,
  nested: FrameOwnerSnapshot[],
): FrameOwnerSnapshot[] {
  const mappings: FrameOwnerSnapshot[] = [
    {
      originalPath: [originalIndex],
      retainedPath: [retainedIndex],
    },
  ];
  for (let index = 0; index < nested.length; index += 1) {
    const originalPath = [originalIndex];
    const retainedPath = [retainedIndex];
    const nestedMapping = nested[index];
    for (
      let pathIndex = 0;
      pathIndex < nestedMapping.originalPath.length;
      pathIndex += 1
    ) {
      arrayPush(originalPath, nestedMapping.originalPath[pathIndex]);
    }
    for (
      let pathIndex = 0;
      pathIndex < nestedMapping.retainedPath.length;
      pathIndex += 1
    ) {
      arrayPush(retainedPath, nestedMapping.retainedPath[pathIndex]);
    }
    arrayPush(mappings, { originalPath, retainedPath });
  }
  return mappings;
}

export function snapshotInlineDocument(
  source: Document,
  request: InlineSnapshotRequest,
): InlineSnapshot {
  const warnings: SnapshotWarning[] = [];
  const visualFallbacks: VisualFallback[] = [];
  const visualFallbackTargets = new SafeMap<string, Element>();
  const snapshot = snapshotDocument(source, {
    warnings,
    visualFallbacks,
    visualFallbackTargets,
    visualFallbackIdPrefix: request.visualFallbackIdPrefix,
    allowScreenshotFallback: true,
    budget: request.budget,
    frameDepth: request.frameDepth,
    nextAnimationMarker: 0,
    nextInlineFallbackNamespace: 0,
    options: request.options,
  });
  return { ...snapshot, warnings, visualFallbacks, visualFallbackTargets };
}

function removeReservedMetadata(root: Element): void {
  const remove: Element[] = [];
  walkElements(root, (element) => {
    if (
      arrayIncludes(
        [
          manifestElementId,
          repairDataElementId,
          repairScriptElementId,
          stateScriptElementId,
        ],
        getAttribute(element, "id") ?? "",
      ) ||
      (hasAttribute(element, animationStyleAttribute) &&
        !isGeneratedMotionStyle(element))
    ) {
      arrayPush(remove, element);
    }
  });
  for (let index = 0; index < remove.length; index += 1) {
    removeNode(remove[index]);
  }
}

function structuralRepairFor(
  root: Element,
  serialized: string,
): StructuralRepairTree | undefined {
  const expected = repairNode(root);
  const reparsed = parseHtml(serialized);
  const actual = repairNode(documentElement(reparsed));
  if (repairNodesEqual(expected, actual)) {
    return undefined;
  }
  return { documentElement: expected };
}

function repairNodesEqual(left: RepairNode, right: RepairNode): boolean {
  const pending: Array<[RepairNode, RepairNode]> = [[left, right]];
  while (pending.length > 0) {
    const pair = arrayPop(pending);
    if (!pair) {
      continue;
    }
    const leftNode = pair[0];
    const rightNode = pair[1];
    if (leftNode.kind !== rightNode.kind) {
      return false;
    }
    if (leftNode.kind === "text" || leftNode.kind === "comment") {
      if (
        rightNode.kind !== leftNode.kind ||
        leftNode.value !== rightNode.value
      ) {
        return false;
      }
      continue;
    }
    if (
      rightNode.kind !== "element" ||
      leftNode.marker !== rightNode.marker ||
      leftNode.namespace !== rightNode.namespace ||
      leftNode.name !== rightNode.name ||
      leftNode.shadowMode !== rightNode.shadowMode ||
      leftNode.children.length !== rightNode.children.length ||
      leftNode.templateContent.length !== rightNode.templateContent.length
    ) {
      return false;
    }
    for (let index = 0; index < leftNode.children.length; index += 1) {
      arrayPush(pending, [leftNode.children[index], rightNode.children[index]]);
    }
    for (let index = 0; index < leftNode.templateContent.length; index += 1) {
      arrayPush(pending, [
        leftNode.templateContent[index],
        rightNode.templateContent[index],
      ]);
    }
  }
  return true;
}

function repairNode(node: Node): RepairNode {
  if (nodeType(node) === 3) {
    return { kind: "text", value: nodeValue(node) ?? "" };
  }
  if (nodeType(node) === 8) {
    return { kind: "comment", value: nodeValue(node) ?? "" };
  }
  if (!isElement(node)) {
    throw new SafeTypeError(
      "structural repair supports element, text, and comment nodes",
    );
  }
  const element = node as Element;
  const marker = getAttribute(element, repairMarkerAttribute) ?? "";
  const children = repairChildren(element);
  const template =
    namespaceUri(element) === "http://www.w3.org/1999/xhtml" &&
    localName(element) === "template"
      ? (element as HTMLTemplateElement)
      : undefined;
  const shadowMode = template
    ? (getAttribute(template, "shadowrootmode") ?? undefined)
    : undefined;
  const repairTemplateContent = template
    ? repairChildren(templateContent(template))
    : [];
  return {
    kind: "element",
    marker,
    namespace: namespaceUri(element) ?? "http://www.w3.org/1999/xhtml",
    name: localName(element) ?? "",
    children,
    templateContent: repairTemplateContent,
    ...(shadowMode ? { shadowMode } : {}),
  };
}

function repairChildren(parent: Node): RepairNode[] {
  const repaired: RepairNode[] = [];
  const children = childNodes(parent);
  for (let index = 0; index < children.length; index += 1) {
    arrayPush(repaired, repairNode(children[index]));
  }
  return repaired;
}

function assignRepairMarkers(root: Element): void {
  let nextMarker = 0;
  walkElements(root, (element) => {
    setAttribute(element, repairMarkerAttribute, SafeString(nextMarker));
    nextMarker += 1;
  });
}

function removeRepairMarkers(root: Element): void {
  walkElements(root, (element) => {
    removeAttribute(element, repairMarkerAttribute);
  });
}

function walkElements(root: Element, visit: (element: Element) => void): void {
  const pending: Element[] = [root];
  while (pending.length > 0) {
    const element = arrayPop(pending);
    if (!element) {
      continue;
    }
    visit(element);
    const children = elementChildren(element);
    if (
      namespaceUri(element) === "http://www.w3.org/1999/xhtml" &&
      localName(element) === "template"
    ) {
      const templateChildren = childNodes(
        templateContent(element as HTMLTemplateElement),
      );
      for (let index = 0; index < templateChildren.length; index += 1) {
        const child = templateChildren[index];
        if (isElement(child)) {
          arrayPush(children, child);
        }
      }
    }
    for (let index = children.length - 1; index >= 0; index -= 1) {
      arrayPush(pending, children[index]);
    }
  }
}

function appendRepairData(
  source: Document,
  root: Element,
  repairData: string,
): void {
  const head = querySelector(root, "head");
  if (!head) {
    throw new SafeTypeError("structural repair requires an HTML head element");
  }
  const data = createElement(source, "script") as HTMLScriptElement;
  setAttribute(data, "id", repairDataElementId);
  setAttribute(data, "type", repairMediaType);
  setNodeTextContent(data, repairData);
  appendChild(head, data);
}

export function doctypeText(source: Document): string {
  const doctype = documentDoctype(source);
  if (!doctype) {
    return "<!doctype html>";
  }
  const publicIdentifier = doctypePublicId(doctype);
  const systemIdentifier = doctypeSystemId(doctype);
  const publicId = publicIdentifier ? ` PUBLIC "${publicIdentifier}"` : "";
  const systemId = systemIdentifier
    ? `${publicIdentifier ? "" : " SYSTEM"} "${systemIdentifier}"`
    : "";
  return `<!DOCTYPE ${doctypeName(doctype)}${publicId}${systemId}>`;
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
