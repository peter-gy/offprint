import {
  attributeAt,
  attributeCount,
  attributeName,
  attributeValue,
  lastChild,
  localName,
  namespaceUri,
  nodeType,
  nodeValue,
  previousSibling,
  templateContent,
} from "./dom";
import {
  arrayIsArray,
  arrayJoin,
  arrayPop,
  arrayPush,
  encodeUtf8Chunks,
  numberIsFinite,
  objectKeys,
  SafeSet,
  setAdd,
  setHas,
  stringCharCodeAt,
  stringSlice,
} from "./primordials";
import { utf8LengthWithinLimit } from "./protocol";

export type BoundedResult<T> =
  | { attempted: number; kind: "limit" }
  | { bytes: number; kind: "ok"; value: T };

class BoundedWriter {
  readonly maximumBytes: number;
  private bytes = 0;
  private readonly chunks: string[] = [];
  private overflow = false;

  constructor(maximumBytes: number) {
    this.maximumBytes = maximumBytes;
  }

  write(value: string): boolean {
    if (this.overflow || value.length === 0) {
      return !this.overflow;
    }
    const width = utf8LengthWithinLimit(value, this.maximumBytes - this.bytes);
    if (width === null) {
      this.overflow = true;
      return false;
    }
    this.bytes += width;
    arrayPush(this.chunks, value);
    return true;
  }

  resultString(): BoundedResult<string> {
    if (this.overflow) {
      return { attempted: this.maximumBytes + 1, kind: "limit" };
    }
    return {
      bytes: this.bytes,
      kind: "ok",
      value: arrayJoin(this.chunks, ""),
    };
  }

  resultBytes(): BoundedResult<Uint8Array> {
    if (this.overflow) {
      return { attempted: this.maximumBytes + 1, kind: "limit" };
    }
    return {
      bytes: this.bytes,
      kind: "ok",
      value: encodeUtf8Chunks(this.chunks, this.bytes),
    };
  }
}

type HtmlTask =
  | { kind: "close"; name: string }
  | { kind: "node"; node: Node; rawText: boolean };

function safeStringSet(values: string[]): Set<string> {
  const result = new SafeSet<string>();
  for (let index = 0; index < values.length; index += 1) {
    setAdd(result, values[index]);
  }
  return result;
}

const voidElements = safeStringSet([
  "area",
  "base",
  "basefont",
  "bgsound",
  "br",
  "col",
  "embed",
  "hr",
  "img",
  "input",
  "link",
  "meta",
  "param",
  "source",
  "track",
  "wbr",
]);
const rawTextElements = safeStringSet([
  "script",
  "style",
  "xmp",
  "iframe",
  "noembed",
  "noframes",
]);

export function serializeHtmlBounded(
  root: Element,
  maximumBytes: number,
): BoundedResult<string> {
  const writer = new BoundedWriter(maximumBytes);
  const pending: HtmlTask[] = [{ kind: "node", node: root, rawText: false }];
  while (pending.length > 0) {
    const task = arrayPop(pending);
    if (!task) {
      continue;
    }
    if (task.kind === "close") {
      if (!writer.write(`</${task.name}>`)) {
        break;
      }
      continue;
    }
    const node = task.node;
    const type = nodeType(node);
    if (type === 3) {
      writeEscaped(
        writer,
        nodeValue(node) ?? "",
        task.rawText ? "raw" : "text",
      );
      continue;
    }
    if (type === 8) {
      if (
        !writer.write("<!--") ||
        !writer.write(nodeValue(node) ?? "") ||
        !writer.write("-->")
      ) {
        break;
      }
      continue;
    }
    if (type !== 1) {
      if (!writer.write(nodeValue(node) ?? "")) {
        break;
      }
      continue;
    }

    const element = node as Element;
    const name = localName(element) ?? "";
    if (!writer.write(`<${name}`)) {
      break;
    }
    const count = attributeCount(element);
    for (let index = 0; index < count; index += 1) {
      const attribute = attributeAt(element, index);
      if (!attribute) {
        continue;
      }
      if (
        !writer.write(` ${attributeName(attribute)}="`) ||
        !writeEscaped(writer, attributeValue(attribute), "attribute") ||
        !writer.write('"')
      ) {
        break;
      }
    }
    if (!writer.write(">")) {
      break;
    }
    const isHtml = namespaceUri(element) === "http://www.w3.org/1999/xhtml";
    if (isHtml && setHas(voidElements, name)) {
      continue;
    }

    arrayPush(pending, { kind: "close", name });
    const rawText = isHtml && setHas(rawTextElements, name);
    const childRoot =
      isHtml && name === "template"
        ? templateContent(element as HTMLTemplateElement)
        : element;
    for (
      let child = lastChild(childRoot);
      child;
      child = previousSibling(child)
    ) {
      arrayPush(pending, { kind: "node", node: child, rawText });
    }
  }
  return writer.resultString();
}

type JsonTask =
  | { kind: "literal"; value: string }
  | { kind: "string"; value: string }
  | { kind: "value"; value: unknown };

export function serializeJsonBytesBounded(
  value: unknown,
  maximumBytes: number,
): BoundedResult<Uint8Array> {
  const writer = new BoundedWriter(maximumBytes);
  writeJson(writer, value);
  return writer.resultBytes();
}

export function serializeJsonStringBounded(
  value: unknown,
  maximumBytes: number,
): BoundedResult<string> {
  const writer = new BoundedWriter(maximumBytes);
  writeJson(writer, value);
  return writer.resultString();
}

export function escapeScriptDataBounded(
  value: string,
  maximumBytes: number,
): BoundedResult<string> {
  const writer = new BoundedWriter(maximumBytes);
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = stringCharCodeAt(value, index);
    const replacement =
      code === 38
        ? "\\u0026"
        : code === 60
          ? "\\u003c"
          : code === 62
            ? "\\u003e"
            : code === 0x2028
              ? "\\u2028"
              : code === 0x2029
                ? "\\u2029"
                : undefined;
    if (replacement === undefined) {
      continue;
    }
    if (
      !writer.write(stringSlice(value, start, index)) ||
      !writer.write(replacement)
    ) {
      return writer.resultString();
    }
    start = index + 1;
  }
  writer.write(stringSlice(value, start));
  return writer.resultString();
}

function writeJson(writer: BoundedWriter, value: unknown): void {
  const pending: JsonTask[] = [{ kind: "value", value }];
  while (pending.length > 0) {
    const task = arrayPop(pending);
    if (!task) {
      continue;
    }
    if (task.kind === "literal") {
      if (!writer.write(task.value)) {
        return;
      }
      continue;
    }
    if (task.kind === "string") {
      if (
        !writer.write('"') ||
        !writeEscaped(writer, task.value, "json") ||
        !writer.write('"')
      ) {
        return;
      }
      continue;
    }

    const current = task.value;
    if (current === null) {
      if (!writer.write("null")) {
        return;
      }
    } else if (typeof current === "string") {
      arrayPush(pending, { kind: "string", value: current });
    } else if (typeof current === "boolean") {
      if (!writer.write(current ? "true" : "false")) {
        return;
      }
    } else if (typeof current === "number") {
      if (!writer.write(numberIsFinite(current) ? `${current}` : "null")) {
        return;
      }
    } else if (arrayIsArray(current)) {
      arrayPush(pending, { kind: "literal", value: "]" });
      for (let index = current.length - 1; index >= 0; index -= 1) {
        arrayPush(pending, { kind: "value", value: current[index] });
        if (index > 0) {
          arrayPush(pending, { kind: "literal", value: "," });
        }
      }
      arrayPush(pending, { kind: "literal", value: "[" });
    } else if (typeof current === "object") {
      const keys = objectKeys(current);
      arrayPush(pending, { kind: "literal", value: "}" });
      for (let index = keys.length - 1; index >= 0; index -= 1) {
        const key = keys[index];
        arrayPush(pending, {
          kind: "value",
          value: (current as Record<string, unknown>)[key],
        });
        arrayPush(pending, { kind: "literal", value: ":" });
        arrayPush(pending, { kind: "string", value: key });
        if (index > 0) {
          arrayPush(pending, { kind: "literal", value: "," });
        }
      }
      arrayPush(pending, { kind: "literal", value: "{" });
    } else if (!writer.write("null")) {
      return;
    }
  }
}

type EscapeMode = "attribute" | "json" | "raw" | "text";

function writeEscaped(
  writer: BoundedWriter,
  value: string,
  mode: EscapeMode,
): boolean {
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = stringCharCodeAt(value, index);
    const replacement =
      mode === "json"
        ? jsonEscape(code)
        : mode === "raw"
          ? undefined
          : htmlEscape(code, mode === "attribute");
    if (replacement === undefined) {
      continue;
    }
    if (
      !writer.write(stringSlice(value, start, index)) ||
      !writer.write(replacement)
    ) {
      return false;
    }
    start = index + 1;
  }
  return writer.write(stringSlice(value, start));
}

function htmlEscape(code: number, attribute: boolean): string | undefined {
  if (code === 38) {
    return "&amp;";
  }
  if (code === 60) {
    return "&lt;";
  }
  if (attribute && code === 34) {
    return "&quot;";
  }
  return undefined;
}

function jsonEscape(code: number): string | undefined {
  switch (code) {
    case 8:
      return "\\b";
    case 9:
      return "\\t";
    case 10:
      return "\\n";
    case 12:
      return "\\f";
    case 13:
      return "\\r";
    case 34:
      return '\\"';
    case 92:
      return "\\\\";
    default:
      return code < 32
        ? `\\u00${"0123456789abcdef"[(code >>> 4) & 0xf]}${
            "0123456789abcdef"[code & 0xf]
          }`
        : undefined;
  }
}
