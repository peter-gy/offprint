import { cssBaseAttribute, documentScrollXAttribute, documentScrollYAttribute } from "./constants";
import {
  appendChild,
  childNodes,
  cloneNode,
  contains,
  createDocumentFragment,
  createTreeWalker,
  capturedTreeWalkerNodes,
  doctypeName,
  doctypePublicId,
  doctypeSystemId,
  documentBody,
  documentDefaultView,
  documentDoctype,
  documentElement,
  getSelection,
  isElement,
  ownerDocument,
  parentNode,
  querySelector,
  querySelectorAll,
  removeAttribute,
  removeNode,
  setAttribute,
  templateContent,
} from "./dom";
import {
  appendMotionStyles,
  freezeMotion,
  materializeMotionState,
  reportMotionCaptureFailure,
} from "./motion";
import { createInspectableShadowTemplate, observedShadowRoot } from "./shadow";
import {
  arrayPop,
  arrayPush,
  mapGet,
  mapSet,
  SafeMap,
  SafeSet,
  SafeString,
  SafeTypeError,
  setAdd,
  setHas,
} from "./primordials";
import { removeReservedMetadata, serializeDocumentWithRepair } from "./repair";
import { applySelectorScope } from "./scope";
import { copyState, prepareElementState, type InlineSnapshotter } from "./state";
import { appendAdoptedStyles, applyCssom } from "./styles";
import type {
  FrameOwnerSnapshot,
  InlineSnapshot,
  SelectionSnapshot,
  SnapshotContext,
  SnapshotRootContext,
  InlineSnapshotRequest,
  SnapshotWarning,
  VisualFallback,
} from "./types";
import {
  computedStyle,
  documentFontFaces,
  rangeCollapsed,
  rangeIntersectsNode,
  selectionRange,
  selectionRangeCount,
  stylePropertyValue,
  treeWalkerCurrent,
  treeWalkerNext,
  windowScrollX,
  windowScrollY,
} from "./web";

function copyShadowRoot(liveRoot: ShadowRoot, cloneHost: Element, context: SnapshotContext): void {
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
      liveChildren.length < cloneChildren.length ? liveChildren.length : cloneChildren.length;
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
      if (context.options.removeHiddenElements && isLayoutlessHiddenElement(liveElement)) {
        removeNode(clone);
      }
    }
  }
  return clones;
}

function isLayoutlessHiddenElement(element: Element): boolean {
  const document = ownerDocument(element);
  const view = document ? documentDefaultView(document) : null;
  const body = document ? documentBody(document) : null;
  return (
    (body ? contains(body, element) : false) &&
    (view ? stylePropertyValue(computedStyle(view, element, null), "display") : "") === "none"
  );
}

function applySelectionScope(source: Document, clones: Map<Node, Node>): SelectionSnapshot {
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
    if (!setHas(selected, node) && parentNode(node) && setHas(selected, parentNode(node) as Node)) {
      const clone = mapGet(clones, node);
      if (clone) {
        removeNode(clone);
      }
    }
  }
  const cloneBody = mapGet(clones, body);
  if (cloneBody && isElement(cloneBody)) {
    setAttribute(cloneBody, "data-offprint-selection", "");
  }
  return {
    ranges: ranges.length,
    nodes: 1 + (cloneBody && isElement(cloneBody) ? querySelectorAll(cloneBody, "*").length : 0),
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
  const reservation = rootContext.budget.reserveDocument(source, rootContext.frameDepth);
  const context: SnapshotContext = {
    ...rootContext,
    documentFontFaces: documentFontFaces(source),
    inlineFrameOwners: new SafeMap<Element, FrameOwnerSnapshot[]>(),
    reservation,
    usedFontsByRoot: new SafeMap<Document | ShadowRoot, Set<string>>(),
  };
  freezeMotion(source);
  reportMotionCaptureFailure(source, context);
  const liveDocumentElement = documentElement(source);
  const clone = cloneNode(liveDocumentElement, true) as Element;
  const motionRules: string[] = [];
  const clones = materializePairs(liveDocumentElement, clone, context, motionRules);
  const view = documentDefaultView(source);
  setAttribute(clone, documentScrollXAttribute, SafeString(view ? windowScrollX(view) : 0));
  setAttribute(clone, documentScrollYAttribute, SafeString(view ? windowScrollY(view) : 0));
  const liveFrameOwners = querySelectorAll(source, "iframe, frame");
  const removableSources = querySelectorAll(clone, "picture source, video source, audio source");
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
  for (let originalIndex = 0; originalIndex < liveFrameOwners.length; originalIndex += 1) {
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
  const maximumDocumentBytes = context.budget.maximumDocumentBytes(reservation);
  const serialized = serializeDocumentWithRepair(source, clone, maximumDocumentBytes);
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
    for (let pathIndex = 0; pathIndex < nestedMapping.originalPath.length; pathIndex += 1) {
      arrayPush(originalPath, nestedMapping.originalPath[pathIndex]);
    }
    for (let pathIndex = 0; pathIndex < nestedMapping.retainedPath.length; pathIndex += 1) {
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
