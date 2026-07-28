import { observedShadowRoot } from "./shadow";
import {
  attributeAt,
  attributeCount,
  attributeName,
  attributeValue,
  doctypeName,
  firstChild,
  localName,
  namespaceUri,
  nextSibling,
  nodeType,
  nodeValue,
  templateContent,
} from "./dom";
import {
  arrayPop,
  arrayPush,
  numberIsSafeInteger,
  SafeTypeError,
  SafeWeakSet,
  weakSetAdd,
  weakSetHas,
} from "./primordials";
import {
  frameDepthError,
  frameLimitError,
  nodeLimitError,
  payloadLimitError,
  utf8LengthWithinLimit,
} from "./protocol";
import type { CollectorProtocolError, SnapshotReservation } from "./types";

const htmlNamespace = "http://www.w3.org/1999/xhtml";
const elementNode = 1;
const documentNode = 9;
const documentFragmentNode = 11;

export interface NodeCursor {
  nodeType: number;
  firstChild: NodeCursor | null;
  nextSibling: NodeCursor | null;
  namespaceURI?: string | null;
  localName?: string | null;
  name?: string;
  nodeValue?: string | null;
  attributes?: Array<{ name: string; value: string }>;
  content?: NodeCursor;
}

export type ShadowLookup = (element: NodeCursor) => NodeCursor | undefined;

type PreflightResult =
  | { kind: "ok"; nodes: number; payloadBytes: number }
  | { attempted: number; kind: "nodes" | "payload" };

export class SnapshotLimitError extends Error {
  readonly response: CollectorProtocolError;

  constructor(response: CollectorProtocolError) {
    super(response.payload.message);
    this.name = "PageKnotSnapshotLimitError";
    this.response = response;
    weakSetAdd(snapshotLimitErrors, this);
  }
}

const snapshotLimitErrors = new SafeWeakSet<SnapshotLimitError>();

export function snapshotLimitResponse(
  error: unknown,
): CollectorProtocolError | undefined {
  if (
    typeof error !== "object" ||
    error === null ||
    !weakSetHas(snapshotLimitErrors, error as SnapshotLimitError)
  ) {
    return undefined;
  }
  return (error as SnapshotLimitError).response;
}

export class RecursiveSnapshotBudget {
  readonly captureId: string;
  readonly maximumFrameDepth: number;
  readonly maximumFrames: number;
  readonly maximumPayloadBytes: number;
  private consumedFrames = 0;
  private remainingFrames: number;
  private remainingNodes: number;
  private remainingPayloadBytes: number;

  constructor(
    captureId: string,
    maximumNodes: number,
    maximumPayloadBytes: number,
    maximumFrames: number,
    maximumFrameDepth: number,
  ) {
    this.captureId = captureId;
    this.maximumFrameDepth = maximumFrameDepth;
    this.maximumFrames = maximumFrames;
    this.maximumPayloadBytes = maximumPayloadBytes;
    this.remainingFrames = maximumFrames;
    this.remainingNodes = maximumNodes;
    this.remainingPayloadBytes = maximumPayloadBytes;
  }

  reserveDocument(source: Document, frameDepth: number): SnapshotReservation {
    if (frameDepth > this.maximumFrameDepth) {
      throw new SnapshotLimitError(
        frameDepthError(this.captureId, frameDepth, this.maximumFrameDepth),
      );
    }
    if (this.remainingFrames < 1) {
      throw new SnapshotLimitError(
        frameLimitError(
          this.captureId,
          this.consumedFrames + 1,
          this.maximumFrames,
        ),
      );
    }
    const measured = preflightDocumentWithinLimits(
      source,
      this.remainingNodes,
      this.remainingPayloadBytes,
    );
    if (measured.kind !== "ok") {
      if (measured.kind === "nodes") {
        throw new SnapshotLimitError(
          nodeLimitError(
            this.captureId,
            measured.attempted,
            this.remainingNodes,
          ),
        );
      }
      throw new SnapshotLimitError(
        payloadLimitError(
          this.captureId,
          this.maximumPayloadBytes,
          this.maximumPayloadBytes -
            this.remainingPayloadBytes +
            measured.attempted,
        ),
      );
    }

    this.consumedFrames += 1;
    this.remainingFrames -= 1;
    this.remainingNodes -= measured.nodes;
    this.remainingPayloadBytes -= measured.payloadBytes;
    return {
      allocatedPayloadBytes: 0,
      nestedFrames: 0,
      nestedNodes: 0,
      nestedPayloadBytes: 0,
      nodes: measured.nodes,
      sourcePayloadBytes: measured.payloadBytes,
    };
  }

  recordNestedSnapshot(
    reservation: SnapshotReservation,
    payloadBytes: number,
    nodes: number,
    frames: number,
  ): void {
    reservation.nestedPayloadBytes += payloadBytes;
    reservation.nestedNodes += nodes;
    reservation.nestedFrames += frames;
  }

  maximumDocumentBytes(reservation: SnapshotReservation): number {
    return (
      reservation.allocatedPayloadBytes +
      reservation.nestedPayloadBytes +
      reservation.sourcePayloadBytes +
      this.remainingPayloadBytes
    );
  }

  maximumPayloadAllocation(_reservation: SnapshotReservation): number {
    return this.remainingPayloadBytes;
  }

  reservePayloadAllocation(
    reservation: SnapshotReservation,
    payloadBytes: number,
  ): void {
    if (!numberIsSafeInteger(payloadBytes) || payloadBytes < 0) {
      throw new SafeTypeError(
        "payloadBytes must be a non-negative safe integer",
      );
    }
    if (payloadBytes > this.remainingPayloadBytes) {
      throw new SnapshotLimitError(
        payloadLimitError(
          this.captureId,
          this.maximumPayloadBytes,
          this.maximumPayloadBytes - this.remainingPayloadBytes + payloadBytes,
        ),
      );
    }
    this.remainingPayloadBytes -= payloadBytes;
    reservation.allocatedPayloadBytes += payloadBytes;
  }

  settlePayloadAllocation(
    reservation: SnapshotReservation,
    reservedBytes: number,
    actualBytes: number,
  ): void {
    if (
      !numberIsSafeInteger(reservedBytes) ||
      reservedBytes < 0 ||
      !numberIsSafeInteger(actualBytes) ||
      actualBytes < 0 ||
      reservedBytes > reservation.allocatedPayloadBytes
    ) {
      throw new SafeTypeError("payload allocation settlement is invalid");
    }
    if (actualBytes > reservedBytes) {
      this.reservePayloadAllocation(reservation, actualBytes - reservedBytes);
      return;
    }
    const released = reservedBytes - actualBytes;
    this.remainingPayloadBytes += released;
    reservation.allocatedPayloadBytes -= released;
  }

  limitResponse(error: unknown): CollectorProtocolError | undefined {
    return snapshotLimitResponse(error);
  }

  commitDocument(reservation: SnapshotReservation, payloadBytes: number): void {
    const documentPayload = payloadBytes - reservation.nestedPayloadBytes;
    const ownPayloadBytes = documentPayload > 0 ? documentPayload : 0;
    const adjustment = ownPayloadBytes - reservation.sourcePayloadBytes;
    const availablePayloadBytes =
      this.remainingPayloadBytes + reservation.allocatedPayloadBytes;
    if (adjustment > availablePayloadBytes) {
      throw new SnapshotLimitError(
        payloadLimitError(
          this.captureId,
          this.maximumPayloadBytes,
          this.maximumPayloadBytes - availablePayloadBytes + adjustment,
        ),
      );
    }
    this.remainingPayloadBytes = availablePayloadBytes - adjustment;
    reservation.allocatedPayloadBytes = 0;
  }

  rejectPayload(attempted: number): never {
    throw new SnapshotLimitError(
      payloadLimitError(this.captureId, this.maximumPayloadBytes, attempted),
    );
  }

  rejectPayloadAllocation(
    _reservation: SnapshotReservation,
    attemptedBytes: number,
  ): never {
    throw new SnapshotLimitError(
      payloadLimitError(
        this.captureId,
        this.maximumPayloadBytes,
        this.maximumPayloadBytes - this.remainingPayloadBytes + attemptedBytes,
      ),
    );
  }
}

export function countCloneableNodesWithinLimit(
  root: NodeCursor | null,
  maximumNodes: number,
  shadowFor: ShadowLookup,
): number {
  if (!numberIsSafeInteger(maximumNodes) || maximumNodes < 0) {
    throw new SafeTypeError("maximumNodes must be a non-negative safe integer");
  }
  if (!root) {
    return 0;
  }

  let nodes = 0;
  const pending: NodeCursor[] = [root];
  while (pending.length > 0) {
    const node = arrayPop(pending);
    if (!node) {
      continue;
    }
    if (
      node.nodeType !== documentNode &&
      node.nodeType !== documentFragmentNode
    ) {
      nodes += 1;
      if (nodes > maximumNodes) {
        return nodes;
      }
    }

    if (node.nextSibling) {
      arrayPush(pending, node.nextSibling);
    }
    if (node.firstChild) {
      arrayPush(pending, node.firstChild);
    }
    if (node.nodeType !== elementNode) {
      continue;
    }
    if (
      node.namespaceURI === htmlNamespace &&
      node.localName === "template" &&
      node.content
    ) {
      arrayPush(pending, node.content);
    }
    const shadow = shadowFor(node);
    if (shadow) {
      arrayPush(pending, shadow);
    }
  }
  return nodes;
}

export function preflightDocumentNodes(
  source: Document,
  maximumNodes: number,
): number {
  const measured = preflightDocumentWithinLimits(
    source,
    maximumNodes,
    9_007_199_254_740_991,
  );
  if (measured.kind === "ok") {
    return measured.nodes;
  }
  return measured.attempted;
}

function preflightDocumentWithinLimits(
  source: Document,
  maximumNodes: number,
  maximumPayloadBytes: number,
): PreflightResult {
  let nodes = 0;
  let payloadBytes = 0;
  const pending: Node[] = [source];
  while (pending.length > 0) {
    const node = arrayPop(pending);
    if (!node) {
      continue;
    }
    if (
      nodeType(node) !== documentNode &&
      nodeType(node) !== documentFragmentNode
    ) {
      nodes += 1;
      if (nodes > maximumNodes) {
        return { attempted: nodes, kind: "nodes" };
      }
      const measured = measureDomNodePayload(
        node,
        maximumPayloadBytes - payloadBytes,
      );
      if (measured === null) {
        return {
          attempted: maximumPayloadBytes + 1,
          kind: "payload",
        };
      }
      payloadBytes += measured;
    }

    const sibling = nextSibling(node);
    if (sibling) {
      arrayPush(pending, sibling);
    }
    const child = firstChild(node);
    if (child) {
      arrayPush(pending, child);
    }
    if (nodeType(node) !== elementNode) {
      continue;
    }
    if (
      namespaceUri(node) === htmlNamespace &&
      localName(node) === "template"
    ) {
      arrayPush(pending, templateContent(node as HTMLTemplateElement));
    }
    const shadow = observedShadowRoot(node as Element);
    if (shadow) {
      arrayPush(pending, shadow);
    }
  }
  return { kind: "ok", nodes, payloadBytes };
}

function measureDomNodePayload(
  node: Node,
  maximumBytes: number,
): number | null {
  let bytes = 0;
  const add = (value: string, fixedBytes = 0): boolean => {
    if (bytes > maximumBytes - fixedBytes) {
      return false;
    }
    bytes += fixedBytes;
    const width = utf8LengthWithinLimit(value, maximumBytes - bytes);
    if (width === null) {
      return false;
    }
    bytes += width;
    return true;
  };

  const type = nodeType(node);
  if (type === elementNode) {
    const element = node as Element;
    const name = localName(element) ?? "";
    if (!add(name, 2)) {
      return null;
    }
    const count = attributeCount(element);
    for (let index = 0; index < count; index += 1) {
      const attribute = attributeAt(element, index);
      if (
        attribute &&
        (!add(attributeName(attribute), 1) ||
          !add(attributeValue(attribute), 3))
      ) {
        return null;
      }
    }
    if (!add(name, 3)) {
      return null;
    }
    return bytes;
  }
  if (type === 8) {
    return add(nodeValue(node) ?? "", 7) ? bytes : null;
  }
  if (type === 10) {
    return add(doctypeName(node as DocumentType), 11) ? bytes : null;
  }
  return add(nodeValue(node) ?? "") ? bytes : null;
}
