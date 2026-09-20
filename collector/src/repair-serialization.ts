import { repairMarkerAttribute } from "./constants";
import {
  firstChild,
  getAttribute,
  isElement,
  localName,
  namespaceUri,
  nextSibling,
  nodeType,
  nodeValue,
  templateContent,
} from "./dom";
import { arrayPop, arrayPush, SafeTypeError } from "./primordials";
import { BoundedWriter, type BoundedResult } from "./serialize";

const htmlNamespace = "http://www.w3.org/1999/xhtml";

type RepairTask =
  | { kind: "node"; node: Node }
  | { kind: "children"; node: ChildNode | null; first: boolean }
  | { kind: "template"; element: Element }
  | { kind: "close"; shadowMode: string | null };

export function serializeRepairDataBounded(
  root: Element,
  maximumBytes: number,
): BoundedResult<string> {
  const writer = new BoundedWriter(maximumBytes);
  const pending: RepairTask[] = [{ kind: "node", node: root }];
  if (!writer.write('{"documentElement":')) {
    return writer.resultString();
  }
  while (pending.length > 0) {
    const task = arrayPop(pending);
    if (!task) {
      continue;
    }
    if (task.kind === "children") {
      if (!task.node) {
        if (!writer.write("]")) break;
      } else {
        if (!task.first && !writer.write(",")) break;
        arrayPush(pending, { kind: "children", node: nextSibling(task.node), first: false });
        arrayPush(pending, { kind: "node", node: task.node });
      }
      continue;
    }
    if (task.kind === "template") {
      if (!writer.write(',"templateContent":[')) break;
      const template =
        namespaceUri(task.element) === htmlNamespace && localName(task.element) === "template";
      arrayPush(pending, {
        kind: "close",
        shadowMode: template ? getAttribute(task.element, "shadowrootmode") : null,
      });
      arrayPush(pending, {
        kind: "children",
        node: template ? firstChild(templateContent(task.element as HTMLTemplateElement)) : null,
        first: true,
      });
      continue;
    }
    if (task.kind === "close") {
      if (
        task.shadowMode &&
        (!writer.write(',"shadowMode":') || !writer.writeJsonString(task.shadowMode, "script-json"))
      )
        break;
      if (!writer.write("}")) break;
      continue;
    }

    const node = task.node;
    const type = nodeType(node);
    if (type === 3 || type === 8) {
      if (
        !writer.write(type === 3 ? '{"kind":"text","value":' : '{"kind":"comment","value":') ||
        !writer.writeJsonString(nodeValue(node) ?? "", "script-json") ||
        !writer.write("}")
      )
        break;
      continue;
    }
    if (!isElement(node)) {
      throw new SafeTypeError("structural repair supports element, text, and comment nodes");
    }
    if (
      !writer.write('{"kind":"element","marker":') ||
      !writer.writeJsonString(getAttribute(node, repairMarkerAttribute) ?? "", "script-json") ||
      !writer.write(',"namespace":') ||
      !writer.writeJsonString(namespaceUri(node) ?? htmlNamespace, "script-json") ||
      !writer.write(',"name":') ||
      !writer.writeJsonString(localName(node) ?? "", "script-json") ||
      !writer.write(',"children":[')
    )
      break;
    // One sibling cursor per ancestor bounds traversal state by emitted depth.
    arrayPush(pending, { kind: "template", element: node });
    arrayPush(pending, { kind: "children", node: firstChild(node), first: true });
  }
  writer.write("}");
  return writer.resultString();
}
