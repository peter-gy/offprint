export type { Capability } from "./identity";

export type CaptureScope = "page" | "selection";

export interface PrepareOptions {
  type: "prepare";
  captureScope: CaptureScope;
  captureId: string;
  frameId: number;
  frameDepth: number;
  maximumChunkBytes: number;
  maximumFrameDepth: number;
  maximumFrames: number;
  maximumNodes: number;
  maximumPayloadBytes: number;
  preservePasswordValues: boolean;
  selector?: string;
  removeHiddenElements: boolean;
  removeUnusedCss: boolean;
  removeUnusedFonts: boolean;
}

export interface StoredObservation {
  captureId: string;
  frameId: number;
  bytes: Uint8Array;
  chunkCount: number;
  maximumChunkBytes: number;
  acknowledged: Set<number>;
  sha256: string;
}

export interface CollectorProtocolError {
  type: "error";
  payload: {
    captureId: string;
    code: string;
    message: string;
    details?: Record<string, number>;
  };
}

export interface SnapshotWarning {
  code: string;
  message: string;
}

export type VisualFallbackKind = "canvas" | "video";

export interface VisualFallback {
  id: string;
  kind: VisualFallbackKind;
  x: string;
  y: string;
  width: string;
  height: string;
}

export interface SnapshotContext {
  warnings: SnapshotWarning[];
  visualFallbacks: VisualFallback[];
  visualFallbackTargets: Map<string, Element>;
  visualFallbackIdPrefix: string;
  allowScreenshotFallback: boolean;
  budget: SnapshotBudget;
  documentFontFaces: Set<string>;
  frameDepth: number;
  inlineFrameOwners: Map<Element, FrameOwnerSnapshot[]>;
  nextAnimationMarker: number;
  nextInlineFallbackNamespace: number;
  options: SnapshotOptions;
  reservation: SnapshotReservation;
  usedFontsByRoot: Map<Document | ShadowRoot, Set<string>>;
}

export interface SnapshotRootContext {
  warnings: SnapshotWarning[];
  visualFallbacks: VisualFallback[];
  visualFallbackTargets: Map<string, Element>;
  visualFallbackIdPrefix: string;
  allowScreenshotFallback: boolean;
  budget: SnapshotBudget;
  frameDepth: number;
  nextAnimationMarker: number;
  nextInlineFallbackNamespace: number;
  options: SnapshotOptions;
  selectorTarget?: Element;
}

export interface SnapshotOptions {
  captureScope: CaptureScope;
  preservePasswordValues: boolean;
  removeHiddenElements: boolean;
  removeUnusedCss: boolean;
  removeUnusedFonts: boolean;
}

export interface SnapshotReservation {
  allocatedPayloadBytes: number;
  nestedFrames: number;
  nestedNodes: number;
  nestedPayloadBytes: number;
  nodes: number;
  sourcePayloadBytes: number;
}

export interface SnapshotBudget {
  commitDocument(reservation: SnapshotReservation, payloadBytes: number): void;
  limitResponse(error: unknown): CollectorProtocolError | undefined;
  maximumDocumentBytes(reservation: SnapshotReservation): number;
  maximumPayloadAllocation(reservation: SnapshotReservation): number;
  recordNestedSnapshot(
    reservation: SnapshotReservation,
    payloadBytes: number,
    nodes: number,
    frames: number,
  ): void;
  rejectPayload(attempted: number): never;
  rejectPayloadAllocation(
    reservation: SnapshotReservation,
    attemptedBytes: number,
  ): never;
  reservePayloadAllocation(
    reservation: SnapshotReservation,
    payloadBytes: number,
  ): void;
  reserveDocument(source: Document, frameDepth: number): SnapshotReservation;
  settlePayloadAllocation(
    reservation: SnapshotReservation,
    reservedBytes: number,
    actualBytes: number,
  ): void;
}

export interface InlineSnapshotRequest {
  budget: SnapshotBudget;
  frameDepth: number;
  options: SnapshotOptions;
  visualFallbackIdPrefix: string;
}

export interface SelectionSnapshot {
  ranges: number;
  nodes: number;
}

export interface FrameOwnerSnapshot {
  originalPath: number[];
  retainedPath: number[];
}

export interface InlineSnapshot {
  frames: number;
  html: string;
  nodes: number;
  subtreeNodes: number;
  payloadBytes: number;
  selection: SelectionSnapshot;
  frameOwners: FrameOwnerSnapshot[];
  warnings: SnapshotWarning[];
  visualFallbacks: VisualFallback[];
  visualFallbackTargets: Map<string, Element>;
}

export type RepairNode =
  | {
      kind: "element";
      marker: string;
      namespace: string;
      name: string;
      children: RepairNode[];
      templateContent: RepairNode[];
      shadowMode?: string;
    }
  | { kind: "text"; value: string }
  | { kind: "comment"; value: string };

export interface StructuralRepairTree {
  documentElement: RepairNode;
}
