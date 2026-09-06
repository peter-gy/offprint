import {
  animationMarkerAttribute,
  documentScrollXAttribute,
  documentScrollYAttribute,
  elementScrollLeftAttribute,
  elementScrollTopAttribute,
} from "./constants";
import {
  attributeAt,
  attributeCount,
  attributeName,
  attributeValue,
  createElement,
  documentBaseUri,
  documentDefaultView,
  documentElement,
  documentScrollingElement,
  elementBounds,
  frameContentDocument,
  frameContentWindow,
  getAttribute,
  isElement,
  localName,
  namespaceUri,
  ownerDocument,
  removeAttribute,
  replaceNode,
  setAttribute,
  setNodeTextContent,
  toggleAttribute,
} from "./dom";
import {
  arrayIncludes,
  arrayPush,
  mapGet,
  mapSet,
  mathCeil,
  numberIsFinite,
  numberIsSafeInteger,
  SafeError,
  SafeNumber,
  SafeString,
  SafeTypeError,
} from "./primordials";
import { utf8LengthWithinLimit } from "./protocol";
import type {
  InlineSnapshotRequest,
  InlineSnapshot,
  SnapshotContext,
  VisualFallbackKind,
} from "./types";
import {
  canvasContext,
  canvasDataUrl,
  canvasHeight,
  canvasWidth,
  detailsOpen,
  drawCanvasImage,
  elementClientHeight,
  elementClientLeft,
  elementClientTop,
  elementClientWidth,
  elementOffsetHeight,
  elementOffsetWidth,
  elementScrollLeft,
  elementScrollTop,
  elementScrollIntoView,
  imageCurrentSource,
  inputChecked,
  inputType,
  inputValue,
  mediaControls,
  mediaCurrentSource,
  mediaCurrentTime,
  mediaMuted,
  mediaReadyState,
  optionSelected,
  rectHeight,
  rectLeft,
  rectTop,
  rectWidth,
  setCanvasHeight,
  setCanvasWidth,
  setImageAlt,
  setImageHeight,
  setImageSource,
  setImageWidth,
  textAreaValue,
  videoHeight,
  videoPoster,
  videoWidth,
  windowScrollX,
  windowScrollY,
  windowFrameElement,
} from "./web";

export type InlineSnapshotter = (
  source: Document,
  request: InlineSnapshotRequest,
) => InlineSnapshot;

export function copyState(
  live: Node,
  clone: Node,
  context: SnapshotContext,
  snapshotInline: InlineSnapshotter,
): void {
  if (isHtmlElement(live, "input") && isHtmlElement(clone, "input")) {
    const liveInput = live as HTMLInputElement;
    const cloneInput = clone as HTMLInputElement;
    if (
      inputType(liveInput) === "password" &&
      !context.options.preservePasswordValues
    ) {
      removeAttribute(cloneInput, "value");
      arrayPush(context.warnings, {
        code: "offprint.form.password_redacted",
        message: "A password value was redacted.",
      });
    } else {
      setAttribute(cloneInput, "value", inputValue(liveInput));
    }
    toggleAttribute(cloneInput, "checked", inputChecked(liveInput));
  } else if (
    isHtmlElement(live, "textarea") &&
    isHtmlElement(clone, "textarea")
  ) {
    setNodeTextContent(clone, textAreaValue(live as HTMLTextAreaElement));
  } else if (isHtmlElement(live, "option") && isHtmlElement(clone, "option")) {
    toggleAttribute(
      clone,
      "selected",
      optionSelected(live as HTMLOptionElement),
    );
  } else if (
    isHtmlElement(live, "details") &&
    isHtmlElement(clone, "details")
  ) {
    toggleAttribute(clone, "open", detailsOpen(live as HTMLDetailsElement));
  } else if (isHtmlElement(live, "img") && isHtmlElement(clone, "img")) {
    const liveImage = live as HTMLImageElement;
    const currentSource = imageCurrentSource(liveImage);
    if (currentSource) {
      setAttribute(clone, "src", currentSource);
      removeAttribute(clone, "srcset");
      removeAttribute(clone, "sizes");
    }
  } else if (isHtmlElement(live, "canvas") && isHtmlElement(clone, "canvas")) {
    const liveCanvas = live as HTMLCanvasElement;
    const cloneCanvas = clone as HTMLCanvasElement;
    let allocatedPayloadBytes = 0;
    try {
      if (!canvasContext(liveCanvas, "2d")) {
        throw new SafeError("canvas pixels require a browser screenshot");
      }
      const encoded = captureCanvasDataUrl(liveCanvas, context);
      allocatedPayloadBytes = encoded.bytes;
      const image = canvasImage(liveCanvas, cloneCanvas);
      setImageSource(image, encoded.value);
      setAttribute(image, "data-offprint-canvas", "");
      replaceNode(cloneCanvas, image);
    } catch (error) {
      if (allocatedPayloadBytes > 0) {
        context.budget.settlePayloadAllocation(
          context.reservation,
          allocatedPayloadBytes,
          0,
        );
      }
      if (context.budget.limitResponse(error)) {
        throw error;
      }
      materializeVisualFallback(liveCanvas, cloneCanvas, "canvas", context);
    }
  } else if (
    isHtmlElement(live, "video", "audio") &&
    isHtmlElement(clone, "video", "audio")
  ) {
    const liveMedia = live as HTMLMediaElement;
    const cloneMedia = clone as HTMLMediaElement;
    setAttribute(
      cloneMedia,
      "data-offprint-current-time",
      SafeString(mediaCurrentTime(liveMedia)),
    );
    toggleAttribute(cloneMedia, "controls", mediaControls(liveMedia));
    toggleAttribute(cloneMedia, "muted", mediaMuted(liveMedia));
    const currentSource = mediaCurrentSource(liveMedia);
    if (currentSource) {
      setAttribute(cloneMedia, "src", currentSource);
    }
    if (isHtmlElement(live, "video") && isHtmlElement(clone, "video")) {
      materializeVideo(
        live as HTMLVideoElement,
        clone as HTMLVideoElement,
        context,
      );
    }
  } else if (isHtmlElement(live, "iframe") && isHtmlElement(clone, "iframe")) {
    copyInlineFrame(live as HTMLIFrameElement, clone, context, snapshotInline);
  }
}

export function prepareElementState(live: Element, clone: Element): void {
  const reserved = [
    documentScrollXAttribute,
    documentScrollYAttribute,
    elementScrollLeftAttribute,
    elementScrollTopAttribute,
    animationMarkerAttribute,
  ];
  for (let index = 0; index < reserved.length; index += 1) {
    removeAttribute(clone, reserved[index]);
  }
  const document = ownerDocument(live);
  if (document && documentScrollingElement(document) === live) {
    return;
  }
  const scrollLeft = elementScrollLeft(live);
  if (numberIsFinite(scrollLeft) && scrollLeft !== 0) {
    setAttribute(clone, elementScrollLeftAttribute, SafeString(scrollLeft));
  }
  const scrollTop = elementScrollTop(live);
  if (numberIsFinite(scrollTop) && scrollTop !== 0) {
    setAttribute(clone, elementScrollTopAttribute, SafeString(scrollTop));
  }
}

function copyInlineFrame(
  liveFrame: HTMLIFrameElement,
  clone: Element,
  context: SnapshotContext,
  snapshotInline: InlineSnapshotter,
): void {
  try {
    const childDocument = frameContentDocument(liveFrame);
    if (!childDocument || !documentElement(childDocument)) {
      return;
    }
    const childWindow = frameContentWindow(liveFrame) as unknown as {
      __offprintCollector?: {
        call(
          method: "snapshotInline",
          arguments_: [InlineSnapshotRequest],
        ): InlineSnapshot;
      };
    } | null;
    const childCollector = childWindow?.__offprintCollector;
    const options = { ...context.options, captureScope: "page" as const };
    const visualFallbackIdPrefix = `${context.visualFallbackIdPrefix}inline-${SafeString(context.nextInlineFallbackNamespace)}-`;
    context.nextInlineFallbackNamespace += 1;
    const request: InlineSnapshotRequest = {
      budget: context.budget,
      frameDepth: context.frameDepth + 1,
      options,
      visualFallbackIdPrefix,
    };
    const child = childCollector
      ? childCollector.call("snapshotInline", [request])
      : snapshotInline(childDocument, request);
    context.budget.recordNestedSnapshot(
      context.reservation,
      child.payloadBytes,
      child.subtreeNodes,
      child.frames,
    );
    mapSet(context.inlineFrameOwners, clone, child.frameOwners);
    setAttribute(
      clone,
      "srcdoc",
      mergeInlineVisualFallbacks(liveFrame, child, context),
    );
    setAttribute(
      clone,
      "data-offprint-frame-base",
      documentBaseUri(childDocument),
    );
    removeAttribute(clone, "src");
    for (let index = 0; index < child.warnings.length; index += 1) {
      arrayPush(context.warnings, child.warnings[index]);
    }
  } catch (error) {
    if (context.budget.limitResponse(error)) {
      throw error;
    }
    arrayPush(context.warnings, {
      code: "offprint.frame.cross_origin",
      message: "A frame requires collection through its attached target.",
    });
  }
}

function mergeInlineVisualFallbacks(
  frame: HTMLIFrameElement,
  child: InlineSnapshot,
  context: SnapshotContext,
): string {
  const bounds = elementBounds(frame);
  const document = ownerDocument(frame);
  const parentWindow = document ? documentDefaultView(document) : null;
  const childWindow = frameContentWindow(frame);
  const offsetWidth = elementOffsetWidth(frame);
  const offsetHeight = elementOffsetHeight(frame);
  const width = rectWidth(bounds);
  const height = rectHeight(bounds);
  const scaleX = offsetWidth > 0 ? width / offsetWidth : 1;
  const scaleY = offsetHeight > 0 ? height / offsetHeight : 1;
  const contentX =
    rectLeft(bounds) +
    (parentWindow ? windowScrollX(parentWindow) : 0) +
    elementClientLeft(frame) * scaleX;
  const contentY =
    rectTop(bounds) +
    (parentWindow ? windowScrollY(parentWindow) : 0) +
    elementClientTop(frame) * scaleY;
  for (let index = 0; index < child.visualFallbacks.length; index += 1) {
    const fallback = child.visualFallbacks[index];
    arrayPush(context.visualFallbacks, {
      ...fallback,
      x: SafeString(
        contentX +
          (SafeNumber(fallback.x) -
            (childWindow ? windowScrollX(childWindow) : 0)) *
            scaleX,
      ),
      y: SafeString(
        contentY +
          (SafeNumber(fallback.y) -
            (childWindow ? windowScrollY(childWindow) : 0)) *
            scaleY,
      ),
      width: SafeString(SafeNumber(fallback.width) * scaleX),
      height: SafeString(SafeNumber(fallback.height) * scaleY),
    });
    const target = mapGet(child.visualFallbackTargets, fallback.id);
    if (target) {
      mapSet(context.visualFallbackTargets, fallback.id, target);
    }
  }
  return child.html;
}

function isHtmlElement(node: Node, ...localNames: string[]): node is Element {
  return (
    isElement(node) &&
    namespaceUri(node) === "http://www.w3.org/1999/xhtml" &&
    arrayIncludes(localNames, localName(node) ?? "")
  );
}

function canvasImage(
  live: HTMLCanvasElement,
  clone: HTMLCanvasElement,
): HTMLImageElement {
  const document = ownerDocument(live);
  if (!document) {
    throw new SafeTypeError("a canvas has no owner document");
  }
  const image = createElement(document, "img") as HTMLImageElement;
  copyAttributes(clone, image);
  setImageWidth(image, canvasWidth(live));
  setImageHeight(image, canvasHeight(live));
  setImageAlt(image, getAttribute(clone, "aria-label") ?? "");
  return image;
}

function videoImage(
  live: HTMLVideoElement,
  clone: HTMLVideoElement,
): HTMLImageElement {
  const document = ownerDocument(live);
  if (!document) {
    throw new SafeTypeError("a video has no owner document");
  }
  const image = createElement(document, "img") as HTMLImageElement;
  copyAttributes(clone, image);
  removeAttribute(image, "src");
  removeAttribute(image, "poster");
  removeAttribute(image, "controls");
  setImageWidth(image, videoWidth(live) || elementClientWidth(live));
  setImageHeight(image, videoHeight(live) || elementClientHeight(live));
  setImageAlt(image, getAttribute(clone, "aria-label") ?? "");
  setAttribute(image, "data-offprint-media", "video");
  return image;
}

function copyAttributes(source: Element, destination: Element): void {
  const count = attributeCount(source);
  for (let index = 0; index < count; index += 1) {
    const attribute = attributeAt(source, index);
    if (attribute) {
      setAttribute(
        destination,
        attributeName(attribute),
        attributeValue(attribute),
      );
    }
  }
}

function materializeVideo(
  live: HTMLVideoElement,
  clone: HTMLVideoElement,
  context: SnapshotContext,
): void {
  const readyState = mediaReadyState(live);
  const width = videoWidth(live);
  const height = videoHeight(live);
  if (readyState >= 2 && width > 0 && height > 0) {
    let reservedPayloadBytes = 0;
    let allocatedPayloadBytes = 0;
    try {
      reservedPayloadBytes = reserveCanvasDataUrl(width, height, context);
      const document = ownerDocument(live);
      if (!document) {
        throw new SafeTypeError("a video has no owner document");
      }
      const canvas = createElement(document, "canvas") as HTMLCanvasElement;
      setCanvasWidth(canvas, width);
      setCanvasHeight(canvas, height);
      const drawing = canvasContext(canvas, "2d");
      if (!drawing) {
        throw new SafeError("2D canvas context is unavailable");
      }
      drawCanvasImage(drawing, live, 0, 0);
      const encoded = encodeReservedCanvasDataUrl(
        canvas,
        reservedPayloadBytes,
        context,
      );
      reservedPayloadBytes = 0;
      allocatedPayloadBytes = encoded.bytes;
      const image = videoImage(live, clone);
      setImageSource(image, encoded.value);
      setAttribute(image, "data-offprint-media-frame", "");
      replaceNode(clone, image);
      return;
    } catch (error) {
      if (reservedPayloadBytes > 0) {
        context.budget.settlePayloadAllocation(
          context.reservation,
          reservedPayloadBytes,
          0,
        );
      }
      if (allocatedPayloadBytes > 0) {
        context.budget.settlePayloadAllocation(
          context.reservation,
          allocatedPayloadBytes,
          0,
        );
      }
      if (context.budget.limitResponse(error)) {
        throw error;
      }
      materializeVisualFallback(live, clone, "video", context);
      return;
    }
  }
  const poster = videoPoster(live);
  if (poster) {
    const image = videoImage(live, clone);
    setImageSource(image, poster);
    setAttribute(image, "data-offprint-media-poster", "");
    replaceNode(clone, image);
    return;
  }
  materializeVisualFallback(live, clone, "video", context);
}

interface EncodedCanvas {
  bytes: number;
  value: string;
}

function captureCanvasDataUrl(
  canvas: HTMLCanvasElement,
  context: SnapshotContext,
): EncodedCanvas {
  const reservedBytes = reserveCanvasDataUrl(
    canvasWidth(canvas),
    canvasHeight(canvas),
    context,
  );
  try {
    return encodeReservedCanvasDataUrl(canvas, reservedBytes, context);
  } catch (error) {
    context.budget.settlePayloadAllocation(
      context.reservation,
      reservedBytes,
      0,
    );
    throw error;
  }
}

function reserveCanvasDataUrl(
  width: number,
  height: number,
  context: SnapshotContext,
): number {
  const maximumBytes = maximumPngDataUrlBytes(width, height);
  const availableBytes = context.budget.maximumPayloadAllocation(
    context.reservation,
  );
  if (maximumBytes === null || maximumBytes > availableBytes) {
    context.budget.rejectPayloadAllocation(
      context.reservation,
      maximumBytes ?? availableBytes + 1,
    );
  }
  context.budget.reservePayloadAllocation(context.reservation, maximumBytes);
  return maximumBytes;
}

function encodeReservedCanvasDataUrl(
  canvas: HTMLCanvasElement,
  reservedBytes: number,
  context: SnapshotContext,
): EncodedCanvas {
  const value = canvasDataUrl(canvas, "image/png");
  const actualBytes = utf8LengthWithinLimit(value, reservedBytes);
  if (actualBytes === null) {
    context.budget.rejectPayloadAllocation(
      context.reservation,
      reservedBytes + 1,
    );
  }
  context.budget.settlePayloadAllocation(
    context.reservation,
    reservedBytes,
    actualBytes,
  );
  return { bytes: actualBytes, value };
}

export function maximumPngDataUrlBytes(
  width: number,
  height: number,
): number | null {
  if (
    !numberIsSafeInteger(width) ||
    width < 0 ||
    !numberIsSafeInteger(height) ||
    height < 0
  ) {
    return null;
  }
  const rowBytes = width * 4 + 1;
  const sourceBytes = rowBytes * height;
  if (!numberIsSafeInteger(rowBytes) || !numberIsSafeInteger(sourceBytes)) {
    return null;
  }
  const deflateBytes =
    sourceBytes +
    mathCeil(sourceBytes / 4096) +
    mathCeil(sourceBytes / 16384) +
    mathCeil(sourceBytes / 33_554_432) +
    13;
  const chunkBytes = mathCeil(deflateBytes / 65_535) * 12;
  const pngBytes = deflateBytes + chunkBytes + 4096;
  const encodedBytes = 22 + mathCeil(pngBytes / 3) * 4;
  return numberIsSafeInteger(encodedBytes) ? encodedBytes : null;
}

function materializeVisualFallback(
  live: HTMLCanvasElement | HTMLVideoElement,
  clone: HTMLCanvasElement | HTMLVideoElement,
  kind: VisualFallbackKind,
  context: SnapshotContext,
): void {
  if (!context.allowScreenshotFallback) {
    arrayPush(context.warnings, {
      code: `offprint.${kind}.capture_unavailable`,
      message: `The ${kind} bitmap could not be collected from an inline frame.`,
    });
    return;
  }
  const bounds = elementBounds(live);
  if (
    !numberIsFinite(rectLeft(bounds)) ||
    !numberIsFinite(rectTop(bounds)) ||
    !numberIsFinite(rectWidth(bounds)) ||
    !numberIsFinite(rectHeight(bounds)) ||
    rectWidth(bounds) <= 0 ||
    rectHeight(bounds) <= 0
  ) {
    arrayPush(context.warnings, {
      code: `offprint.${kind}.empty_bounds`,
      message: `The ${kind} bitmap has no visible capture bounds.`,
    });
    return;
  }
  const id =
    context.visualFallbackIdPrefix + SafeString(context.visualFallbacks.length);
  const image =
    isHtmlElement(live, "canvas") && isHtmlElement(clone, "canvas")
      ? canvasImage(live as HTMLCanvasElement, clone as HTMLCanvasElement)
      : videoImage(live as HTMLVideoElement, clone as HTMLVideoElement);
  setAttribute(image, "data-offprint-visual-fallback", id);
  setAttribute(image, `data-offprint-${kind}`, "");
  replaceNode(clone, image);
  mapSet(context.visualFallbackTargets, id, live);
  const document = ownerDocument(live);
  const view = document ? documentDefaultView(document) : null;
  arrayPush(context.visualFallbacks, {
    id,
    kind,
    x: SafeString(rectLeft(bounds) + (view ? windowScrollX(view) : 0)),
    y: SafeString(rectTop(bounds) + (view ? windowScrollY(view) : 0)),
    width: SafeString(rectWidth(bounds)),
    height: SafeString(rectHeight(bounds)),
  });
}

export function positionVisualFallback(target: Element): {
  x: string;
  y: string;
  width: string;
  height: string;
} {
  elementScrollIntoView(target, {
    block: "center",
    inline: "center",
  });
  let document = ownerDocument(target);
  let view = document ? documentDefaultView(document) : null;
  let frame = view ? windowFrameElement(view) : null;
  while (frame) {
    elementScrollIntoView(frame, {
      block: "center",
      inline: "center",
    });
    document = ownerDocument(frame);
    view = document ? documentDefaultView(document) : null;
    frame = view ? windowFrameElement(view) : null;
  }

  let bounds = elementBounds(target);
  let x = rectLeft(bounds);
  let y = rectTop(bounds);
  let width = rectWidth(bounds);
  let height = rectHeight(bounds);
  document = ownerDocument(target);
  view = document ? documentDefaultView(document) : null;
  frame = view ? windowFrameElement(view) : null;
  while (frame) {
    bounds = elementBounds(frame);
    const offsetWidth = elementOffsetWidth(frame as HTMLElement);
    const offsetHeight = elementOffsetHeight(frame as HTMLElement);
    const scaleX = offsetWidth > 0 ? rectWidth(bounds) / offsetWidth : 1;
    const scaleY = offsetHeight > 0 ? rectHeight(bounds) / offsetHeight : 1;
    x = rectLeft(bounds) + elementClientLeft(frame) * scaleX + x * scaleX;
    y = rectTop(bounds) + elementClientTop(frame) * scaleY + y * scaleY;
    width *= scaleX;
    height *= scaleY;
    document = ownerDocument(frame);
    view = document ? documentDefaultView(document) : null;
    frame = view ? windowFrameElement(view) : null;
  }
  return {
    x: SafeString(x + (view ? windowScrollX(view) : 0)),
    y: SafeString(y + (view ? windowScrollY(view) : 0)),
    width: SafeString(width),
    height: SafeString(height),
  };
}
