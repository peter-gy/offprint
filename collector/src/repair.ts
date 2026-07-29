import {
  animationStyleAttribute,
  manifestElementId,
  repairDataElementId,
  repairMarkerAttribute,
  repairMediaType,
  repairScriptElementId,
  stateScriptElementId,
} from "./constants";
import {
  appendChild,
  childNodes,
  createElement,
  documentElement,
  elementChildren,
  getAttribute,
  hasAttribute,
  isElement,
  localName,
  namespaceUri,
  nodeType,
  nodeValue,
  parseHtml,
  querySelector,
  removeAttribute,
  removeNode,
  setAttribute,
  setNodeTextContent,
  templateContent,
} from "./dom";
import { isGeneratedMotionStyle } from "./motion";
import {
  arrayIncludes,
  arrayPop,
  arrayPush,
  SafeString,
  SafeTypeError,
} from "./primordials";
import {
  type BoundedResult,
  escapeScriptDataBounded,
  serializeHtmlBounded,
  serializeJsonStringBounded,
} from "./serialize";
import type { RepairNode, StructuralRepairTree } from "./types";

const htmlNamespace = "http://www.w3.org/1999/xhtml";

export function removeReservedMetadata(root: Element): void {
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

export function serializeDocumentWithRepair(
  source: Document,
  root: Element,
  maximumBytes: number,
): BoundedResult<string> {
  assignRepairMarkers(root);
  const initial = serializeHtmlBounded(root, maximumBytes);
  if (initial.kind === "limit") {
    return initial;
  }
  const structuralRepair = structuralRepairFor(root, initial.value);
  if (structuralRepair) {
    const repairJson = serializeJsonStringBounded(
      structuralRepair,
      maximumBytes,
    );
    if (repairJson.kind === "limit") {
      return repairJson;
    }
    const repairData = escapeScriptDataBounded(repairJson.value, maximumBytes);
    if (repairData.kind === "limit") {
      return repairData;
    }
    appendRepairData(source, root, repairData.value);
  } else {
    removeRepairMarkers(root);
  }
  return serializeHtmlBounded(root, maximumBytes);
}

function structuralRepairFor(
  root: Element,
  serialized: string,
): StructuralRepairTree | undefined {
  const reparsed = parseHtml(serialized);
  if (repairNodesEqual(root, documentElement(reparsed))) {
    return undefined;
  }
  return { documentElement: repairNode(root) };
}

function repairNodesEqual(left: Node, right: Node): boolean {
  const pending: Array<[Node, Node]> = [[left, right]];
  while (pending.length > 0) {
    const pair = arrayPop(pending);
    if (!pair) {
      continue;
    }
    const leftNode = pair[0];
    const rightNode = pair[1];
    const leftType = nodeType(leftNode);
    if (leftType !== nodeType(rightNode)) {
      return false;
    }
    if (leftType === 3 || leftType === 8) {
      if ((nodeValue(leftNode) ?? "") !== (nodeValue(rightNode) ?? "")) {
        return false;
      }
      continue;
    }
    if (!isElement(leftNode) || !isElement(rightNode)) {
      return false;
    }
    if (
      getAttribute(leftNode, repairMarkerAttribute) !==
        getAttribute(rightNode, repairMarkerAttribute) ||
      (namespaceUri(leftNode) ?? htmlNamespace) !==
        (namespaceUri(rightNode) ?? htmlNamespace) ||
      (localName(leftNode) ?? "") !== (localName(rightNode) ?? "") ||
      shadowModeFor(leftNode) !== shadowModeFor(rightNode)
    ) {
      return false;
    }
    const leftChildren = childNodes(leftNode);
    const rightChildren = childNodes(rightNode);
    if (leftChildren.length !== rightChildren.length) {
      return false;
    }
    for (let index = 0; index < leftChildren.length; index += 1) {
      arrayPush(pending, [leftChildren[index], rightChildren[index]]);
    }
    const leftTemplate = templateChildren(leftNode);
    const rightTemplate = templateChildren(rightNode);
    if (leftTemplate.length !== rightTemplate.length) {
      return false;
    }
    for (let index = 0; index < leftTemplate.length; index += 1) {
      arrayPush(pending, [leftTemplate[index], rightTemplate[index]]);
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
  const marker = getAttribute(node, repairMarkerAttribute) ?? "";
  const children = repairChildren(node);
  const shadowMode = shadowModeFor(node);
  return {
    kind: "element",
    marker,
    namespace: namespaceUri(node) ?? htmlNamespace,
    name: localName(node) ?? "",
    children,
    templateContent: repairNodes(templateChildren(node)),
    ...(shadowMode ? { shadowMode } : {}),
  };
}

function repairChildren(parent: Node): RepairNode[] {
  return repairNodes(childNodes(parent));
}

function repairNodes(nodes: Node[]): RepairNode[] {
  const repaired: RepairNode[] = [];
  for (let index = 0; index < nodes.length; index += 1) {
    arrayPush(repaired, repairNode(nodes[index]));
  }
  return repaired;
}

function templateChildren(node: Element): ChildNode[] {
  return isHtmlTemplate(node)
    ? childNodes(templateContent(node as HTMLTemplateElement))
    : [];
}

function shadowModeFor(node: Element): string | undefined {
  return isHtmlTemplate(node)
    ? (getAttribute(node, "shadowrootmode") ?? undefined)
    : undefined;
}

function isHtmlTemplate(node: Element): boolean {
  return namespaceUri(node) === htmlNamespace && localName(node) === "template";
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
    const template = templateChildren(element);
    for (let index = 0; index < template.length; index += 1) {
      const child = template[index];
      if (isElement(child)) {
        arrayPush(children, child);
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
