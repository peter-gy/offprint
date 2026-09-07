import { doctypeText, snapshotDocument, snapshotInlineDocument } from "./collection";
import { RecursiveSnapshotBudget, snapshotLimitResponse } from "./budget";
import { availableCapabilities, buildSha256, protocol } from "./constants";
import { dispatchCollector } from "./dispatch";
import { documentBaseUri, documentCharacterSet, documentTitle } from "./dom";
import { freezeDocument } from "./motion";
import {
  arrayPush,
  defineProperty,
  mapDelete,
  mapGet,
  mapSet,
  mathCeil,
  numberIsSafeInteger,
  objectFreeze,
  SafeMap,
  SafeRangeError,
  SafeSet,
  SafeString,
  SafeTypeError,
  setAdd,
  typedArrayByteLength,
  typedArraySubarray,
} from "./primordials";
import {
  crc32,
  frameDepthError,
  frameLimitError,
  nodeLimitError,
  payloadLimitError,
  sha256,
} from "./protocol";
import { resolveSelector, snapshotOptionsFor } from "./scope";
import { serializeJsonBytesBounded } from "./serialize";
import { fallbackTargets } from "./visual-fallback";
import type {
  Capability,
  InlineSnapshot,
  InlineSnapshotRequest,
  PrepareOptions,
  SnapshotWarning,
  StoredObservation,
  VisualFallback,
} from "./types";
import {
  locationHref,
  windowDeviceScaleFactor,
  windowInnerHeight,
  windowInnerWidth,
  windowScrollX,
  windowScrollY,
} from "./web";

const observations = new SafeMap<string, StoredObservation>();

function key(captureId: string, frameId: number): string {
  return `${captureId}:${frameId}`;
}

const collector = objectFreeze({
  call(method: string, arguments_: unknown[]) {
    return dispatchCollector(collector, method, arguments_);
  },

  handshake(
    captureId: string,
    hostBuildSha256: string,
    requestedCapabilities: Capability[],
    maximumChunkBytes: number,
  ) {
    return {
      protocol,
      captureId,
      hostBuildSha256,
      collectorBuildSha256: buildSha256,
      requestedCapabilities,
      availableCapabilities,
      maximumChunkBytes,
    };
  },

  freeze() {
    return freezeDocument(document);
  },

  async prepare(options: PrepareOptions) {
    mapDelete(observations, key(options.captureId, options.frameId));
    fallbackTargets.clear();
    if (!numberIsSafeInteger(options.maximumChunkBytes) || options.maximumChunkBytes < 1) {
      throw new SafeTypeError("maximumChunkBytes must be a positive safe integer");
    }
    if (!numberIsSafeInteger(options.maximumPayloadBytes) || options.maximumPayloadBytes < 1) {
      return payloadLimitError(options.captureId, options.maximumPayloadBytes);
    }
    if (!numberIsSafeInteger(options.maximumNodes) || options.maximumNodes < 0) {
      return nodeLimitError(options.captureId, 0, options.maximumNodes);
    }
    if (!numberIsSafeInteger(options.maximumFrames) || options.maximumFrames < 0) {
      return frameLimitError(options.captureId, 0, options.maximumFrames);
    }
    if (
      !numberIsSafeInteger(options.frameDepth) ||
      options.frameDepth < 0 ||
      !numberIsSafeInteger(options.maximumFrameDepth) ||
      options.maximumFrameDepth < 0
    ) {
      return frameDepthError(options.captureId, options.frameDepth, options.maximumFrameDepth);
    }
    const budget = new RecursiveSnapshotBudget(
      options.captureId,
      options.maximumNodes,
      options.maximumPayloadBytes,
      options.maximumFrames,
      options.maximumFrameDepth,
    );
    const warnings: SnapshotWarning[] = [];
    const visualFallbacks: VisualFallback[] = [];
    const selector = resolveSelector(document, options);
    if (selector.error) return selector.error;
    const snapshotOptions = snapshotOptionsFor(options);
    let snapshot;
    try {
      snapshot = snapshotDocument(document, {
        warnings,
        visualFallbacks,
        visualFallbackTargets: fallbackTargets.map,
        visualFallbackIdPrefix: "",
        allowScreenshotFallback: true,
        budget,
        frameDepth: options.frameDepth,
        nextAnimationMarker: 0,
        nextInlineFallbackNamespace: 0,
        options: snapshotOptions,
        selectorTarget: selector.target,
      });
    } catch (error) {
      const response = snapshotLimitResponse(error);
      if (response) {
        return response;
      }
      throw error;
    }
    const observation = {
      doctype: doctypeText(document),
      html: snapshot.html,
      requestedUrl: locationHref(location),
      finalUrl: locationHref(location),
      baseUrl: documentBaseUri(document),
      title: documentTitle(document),
      encoding: documentCharacterSet(document),
      viewport: {
        width: windowInnerWidth(window),
        height: windowInnerHeight(window),
        deviceScaleFactor: SafeString(windowDeviceScaleFactor(window)),
        scrollX: SafeString(windowScrollX(window)),
        scrollY: SafeString(windowScrollY(window)),
      },
      frames: snapshot.frames,
      nodes: snapshot.nodes,
      subtreeNodes: snapshot.subtreeNodes,
      warnings,
      visualFallbacks,
      selection: snapshot.selection,
      frameOwners: snapshot.frameOwners,
    };
    const serialized = serializeJsonBytesBounded(observation, options.maximumPayloadBytes);
    if (serialized.kind === "limit") {
      return payloadLimitError(
        options.captureId,
        options.maximumPayloadBytes,
        serialized.attempted,
      );
    }
    const bytes = serialized.value;
    const calculatedChunks = mathCeil(typedArrayByteLength(bytes) / options.maximumChunkBytes);
    const chunkCount = calculatedChunks > 1 ? calculatedChunks : 1;
    const stored: StoredObservation = {
      captureId: options.captureId,
      frameId: options.frameId,
      bytes,
      chunkCount,
      maximumChunkBytes: options.maximumChunkBytes,
      acknowledged: new SafeSet(),
      sha256: await sha256(bytes),
    };
    mapSet(observations, key(options.captureId, options.frameId), stored);
    return collector.describe(options.captureId, options.frameId);
  },

  snapshotInline(request: InlineSnapshotRequest): InlineSnapshot {
    return snapshotInlineDocument(document, request);
  },

  positionVisualFallback: fallbackTargets.position,

  describe(captureId: string, frameId: number) {
    const stored = mapGet(observations, key(captureId, frameId));
    if (!stored) {
      throw new SafeTypeError("observation is not prepared");
    }
    return {
      captureId,
      frameId,
      chunks: stored.chunkCount,
      encodedBytes: typedArrayByteLength(stored.bytes),
      payloadSha256: stored.sha256,
    };
  },

  read(captureId: string, frameId: number, sequence: number) {
    const stored = mapGet(observations, key(captureId, frameId));
    if (
      !stored ||
      !numberIsSafeInteger(sequence) ||
      sequence < 0 ||
      sequence >= stored.chunkCount
    ) {
      throw new SafeRangeError("observation chunk is unavailable");
    }
    const offset = sequence * stored.maximumChunkBytes;
    const payload = typedArraySubarray(stored.bytes, offset, offset + stored.maximumChunkBytes);
    const payloadValues: number[] = [];
    for (let index = 0; index < typedArrayByteLength(payload); index += 1) {
      arrayPush(payloadValues, payload[index]);
    }
    return {
      captureId,
      frameId,
      sequence,
      total: stored.chunkCount,
      payloadLength: typedArrayByteLength(payload),
      payloadCrc32: crc32(payload),
      payload: payloadValues,
    };
  },

  acknowledge(captureId: string, frameId: number, sequence: number) {
    const stored = mapGet(observations, key(captureId, frameId));
    if (
      !stored ||
      !numberIsSafeInteger(sequence) ||
      sequence < 0 ||
      sequence >= stored.chunkCount
    ) {
      throw new SafeRangeError("observation chunk is unavailable");
    }
    setAdd(stored.acknowledged, sequence);
    return true;
  },

  release(captureId: string, frameId: number) {
    mapDelete(observations, key(captureId, frameId));
    return { captureId, frameId };
  },
});
defineProperty(globalThis, "__offprintCollector", {
  configurable: false,
  enumerable: false,
  writable: false,
  value: collector,
});
declare global {
  var __offprintCollector: typeof collector;
}
