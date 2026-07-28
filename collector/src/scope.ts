import {
  childNodes,
  contains,
  documentBody,
  documentElement,
  parentNode,
  querySelector,
  removeNode,
} from "./dom";
import { mapGet, SafeTypeError } from "./primordials";
import { selectorInvalidError, selectorNotFoundError } from "./protocol";
import type {
  CollectorProtocolError,
  PrepareOptions,
  SnapshotOptions,
} from "./types";

export function resolveSelector(
  source: Document,
  options: PrepareOptions,
): { error?: CollectorProtocolError; target?: Element } {
  if (options.selector === undefined) {
    return {};
  }
  let target: Element | null;
  try {
    target = querySelector(source, options.selector);
  } catch {
    return { error: selectorInvalidError(options.captureId) };
  }
  return target
    ? { target }
    : { error: selectorNotFoundError(options.captureId) };
}

export function snapshotOptionsFor(options: PrepareOptions): SnapshotOptions {
  return {
    captureScope: options.captureScope,
    preservePasswordValues: options.preservePasswordValues,
    removeHiddenElements: options.removeHiddenElements,
    removeUnusedCss: options.removeUnusedCss,
    removeUnusedFonts: options.removeUnusedFonts,
  };
}

function retainCloneBranch(target: Node, boundary: Node): void {
  let retained = target;
  while (retained !== boundary) {
    const parent = parentNode(retained);
    if (!parent) {
      throw new SafeTypeError("selector target clone is detached");
    }
    const siblings = childNodes(parent);
    for (let index = 0; index < siblings.length; index += 1) {
      if (siblings[index] !== retained) {
        removeNode(siblings[index]);
      }
    }
    retained = parent;
  }
}

function removeCloneChildren(node: Node): void {
  const children = childNodes(node);
  for (let index = 0; index < children.length; index += 1) {
    removeNode(children[index]);
  }
}

export function applySelectorScope(
  source: Document,
  target: Element,
  cloneRoot: Element,
  clones: Map<Node, Node>,
): void {
  const targetClone = mapGet(clones, target);
  if (!targetClone || !contains(cloneRoot, targetClone)) {
    throw new SafeTypeError("selector target clone is unavailable");
  }
  const sourceRoot = documentElement(source);
  if (target === sourceRoot) {
    return;
  }

  const body = documentBody(source);
  if (body && contains(body, target)) {
    const bodyClone = mapGet(clones, body);
    if (!bodyClone) {
      throw new SafeTypeError("document body clone is unavailable");
    }
    retainCloneBranch(targetClone, bodyClone);
    return;
  }

  const head = querySelector(source, "head");
  if (head && contains(head, target)) {
    const headClone = mapGet(clones, head);
    if (!headClone) {
      throw new SafeTypeError("document head clone is unavailable");
    }
    retainCloneBranch(targetClone, headClone);
    if (body) {
      const bodyClone = mapGet(clones, body);
      if (bodyClone) {
        removeCloneChildren(bodyClone);
      }
    }
    return;
  }

  retainCloneBranch(targetClone, cloneRoot);
}
