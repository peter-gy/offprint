import { localName, namespaceUri, styleSheets } from "./dom";
import {
  arrayPush,
  captureGetter,
  captureMethod,
  captureOptionalGetter,
  captureSetter,
  SafeSet,
  setAdd,
} from "./primordials";

const styleSheetListPrototype =
  typeof StyleSheetList === "undefined" ? undefined : StyleSheetList.prototype;
const cssRuleListPrototype = typeof CSSRuleList === "undefined" ? undefined : CSSRuleList.prototype;
const cssStyleSheetPrototype =
  typeof CSSStyleSheet === "undefined" ? undefined : CSSStyleSheet.prototype;
const styleSheetPrototype = typeof StyleSheet === "undefined" ? undefined : StyleSheet.prototype;
const mediaListPrototype = typeof MediaList === "undefined" ? undefined : MediaList.prototype;
const cssRulePrototype = typeof CSSRule === "undefined" ? undefined : CSSRule.prototype;
const cssStyleRulePrototype =
  typeof CSSStyleRule === "undefined" ? undefined : CSSStyleRule.prototype;
const cssFontFaceRulePrototype =
  typeof CSSFontFaceRule === "undefined" ? undefined : CSSFontFaceRule.prototype;
const cssStyleDeclarationPrototype =
  typeof CSSStyleDeclaration === "undefined" ? undefined : CSSStyleDeclaration.prototype;
const windowPrototype = typeof Window === "undefined" ? undefined : Window.prototype;
const windowObject = typeof window === "undefined" ? undefined : window;
const windowPropertySource = windowObject ?? windowPrototype;
const animationPrototype = typeof Animation === "undefined" ? undefined : Animation.prototype;
const keyframeEffectPrototype =
  typeof KeyframeEffect === "undefined" ? undefined : KeyframeEffect.prototype;
const elementPrototype = typeof Element === "undefined" ? undefined : Element.prototype;
const htmlElementPrototype = typeof HTMLElement === "undefined" ? undefined : HTMLElement.prototype;
const styleElementPrototype =
  typeof HTMLStyleElement === "undefined" ? undefined : HTMLStyleElement.prototype;
const svgStyleElementPrototype =
  typeof SVGStyleElement === "undefined" ? undefined : SVGStyleElement.prototype;
const linkElementPrototype =
  typeof HTMLLinkElement === "undefined" ? undefined : HTMLLinkElement.prototype;
const inputPrototype =
  typeof HTMLInputElement === "undefined" ? undefined : HTMLInputElement.prototype;
const textAreaPrototype =
  typeof HTMLTextAreaElement === "undefined" ? undefined : HTMLTextAreaElement.prototype;
const optionPrototype =
  typeof HTMLOptionElement === "undefined" ? undefined : HTMLOptionElement.prototype;
const detailsPrototype =
  typeof HTMLDetailsElement === "undefined" ? undefined : HTMLDetailsElement.prototype;
const imagePrototype =
  typeof HTMLImageElement === "undefined" ? undefined : HTMLImageElement.prototype;
const canvasPrototype =
  typeof HTMLCanvasElement === "undefined" ? undefined : HTMLCanvasElement.prototype;
const mediaPrototype =
  typeof HTMLMediaElement === "undefined" ? undefined : HTMLMediaElement.prototype;
const videoPrototype =
  typeof HTMLVideoElement === "undefined" ? undefined : HTMLVideoElement.prototype;
const selectionPrototype = typeof Selection === "undefined" ? undefined : Selection.prototype;
const rangePrototype = typeof Range === "undefined" ? undefined : Range.prototype;
const treeWalkerPrototype = typeof TreeWalker === "undefined" ? undefined : TreeWalker.prototype;
const context2dPrototype =
  typeof CanvasRenderingContext2D === "undefined" ? undefined : CanvasRenderingContext2D.prototype;
const domRectPrototype =
  typeof DOMRectReadOnly === "undefined" ? undefined : DOMRectReadOnly.prototype;
const locationPrototype = typeof Location === "undefined" ? undefined : Location.prototype;
const locationObject = typeof location === "undefined" ? undefined : location;
const locationPropertySource = locationObject ?? locationPrototype;

const styleSheetListLength = captureGetter<StyleSheetList, number>(
  styleSheetListPrototype,
  "length",
  (list) => list.length,
);
const styleSheetListItem = captureMethod<StyleSheetList, [number], CSSStyleSheet | null>(
  styleSheetListPrototype,
  "item",
  (list, index) => list.item(index) as CSSStyleSheet | null,
);
const cssRuleListLength = captureGetter<CSSRuleList, number>(
  cssRuleListPrototype,
  "length",
  (list) => list.length,
);
const cssRuleListItem = captureMethod<CSSRuleList, [number], CSSRule | null>(
  cssRuleListPrototype,
  "item",
  (list, index) => list.item(index),
);
const getCssRules = captureGetter<CSSStyleSheet, CSSRuleList>(
  cssStyleSheetPrototype,
  "cssRules",
  (sheet) => sheet.cssRules,
);
const getStyleSheetOwner = captureGetter<StyleSheet, Node | null>(
  styleSheetPrototype,
  "ownerNode",
  (sheet) => sheet.ownerNode,
);
const getStyleSheetHref = captureGetter<StyleSheet, string | null>(
  styleSheetPrototype,
  "href",
  (sheet) => sheet.href,
);
export const styleSheetDisabled = captureGetter<StyleSheet, boolean>(
  styleSheetPrototype,
  "disabled",
  (sheet) => sheet.disabled,
);
const getStyleSheetMedia = captureGetter<StyleSheet, MediaList>(
  styleSheetPrototype,
  "media",
  (sheet) => sheet.media,
);
const getMediaText = captureGetter<MediaList, string>(
  mediaListPrototype,
  "mediaText",
  (media) => media.mediaText,
);
export function styleSheetMedia(sheet: CSSStyleSheet): string {
  return getMediaText(getStyleSheetMedia(sheet));
}
const getRuleType = captureGetter<CSSRule, number>(cssRulePrototype, "type", (rule) => rule.type);
const getRuleText = captureGetter<CSSRule, string>(
  cssRulePrototype,
  "cssText",
  (rule) => rule.cssText,
);
const getSelectorText = captureGetter<CSSStyleRule, string>(
  cssStyleRulePrototype,
  "selectorText",
  (rule) => rule.selectorText,
);
const getStyleRuleDeclaration = captureGetter<CSSStyleRule, CSSStyleDeclaration>(
  cssStyleRulePrototype,
  "style",
  (rule) => rule.style,
);
const getFontFaceDeclaration = captureGetter<CSSFontFaceRule, CSSStyleDeclaration>(
  cssFontFaceRulePrototype,
  "style",
  (rule) => rule.style,
);
const getStyleSheetFromStyle = captureGetter<HTMLStyleElement, CSSStyleSheet | null>(
  styleElementPrototype,
  "sheet",
  (element) => element.sheet,
);
const getStyleSheetFromSvgStyle = captureGetter<SVGStyleElement, CSSStyleSheet | null>(
  svgStyleElementPrototype,
  "sheet",
  (element) => element.sheet,
);
const getStyleSheetFromLink = captureGetter<HTMLLinkElement, CSSStyleSheet | null>(
  linkElementPrototype,
  "sheet",
  (element) => element.sheet,
);

export function styleSheetsFromList(list: StyleSheetList): CSSStyleSheet[] {
  const sheets: CSSStyleSheet[] = [];
  const length = styleSheetListLength(list);
  for (let index = 0; index < length; index += 1) {
    const sheet = styleSheetListItem(list, index);
    if (sheet) {
      arrayPush(sheets, sheet);
    }
  }
  return sheets;
}

export function rulesForStyleSheet(sheet: CSSStyleSheet): CSSRuleList {
  return getCssRules(sheet);
}

export function cssRuleCount(list: CSSRuleList): number {
  return cssRuleListLength(list);
}

export function cssRuleAt(list: CSSRuleList, index: number): CSSRule | null {
  return cssRuleListItem(list, index);
}

export function styleSheetOwner(sheet: CSSStyleSheet): Node | null {
  return getStyleSheetOwner(sheet);
}

export function styleSheetHref(sheet: CSSStyleSheet): string | null {
  return getStyleSheetHref(sheet);
}

export function documentFontFaces(source: Document): Set<string> {
  const faces = new SafeSet<string>();
  const sheets = styleSheetsFromList(styleSheets(source));
  for (let sheetIndex = 0; sheetIndex < sheets.length; sheetIndex += 1) {
    const sheet = sheets[sheetIndex];
    if (!styleSheetHref(sheet)) {
      continue;
    }
    try {
      const rules = rulesForStyleSheet(sheet);
      const count = cssRuleCount(rules);
      for (let ruleIndex = 0; ruleIndex < count; ruleIndex += 1) {
        const rule = cssRuleAt(rules, ruleIndex);
        if (rule && cssRuleType(rule) === 5) {
          setAdd(faces, cssRuleText(rule));
        }
      }
    } catch {}
  }
  return faces;
}

export function styleSheetFor(element: Element): CSSStyleSheet | null {
  const name = localName(element);
  const namespace = namespaceUri(element);
  if (name === "style") {
    if (namespace === "http://www.w3.org/1999/xhtml") {
      return getStyleSheetFromStyle(element as HTMLStyleElement);
    }
    if (namespace === "http://www.w3.org/2000/svg") {
      return getStyleSheetFromSvgStyle(element as SVGStyleElement);
    }
    return null;
  }
  return name === "link" && namespace === "http://www.w3.org/1999/xhtml"
    ? getStyleSheetFromLink(element as HTMLLinkElement)
    : null;
}

export function cssRuleType(rule: CSSRule): number {
  return getRuleType(rule);
}

export function cssRuleText(rule: CSSRule): string {
  return getRuleText(rule);
}

export function cssRuleSelector(rule: CSSStyleRule): string {
  return getSelectorText(rule);
}

export function cssRuleDeclaration(rule: CSSStyleRule | CSSFontFaceRule): CSSStyleDeclaration {
  return cssRuleType(rule) === 1
    ? getStyleRuleDeclaration(rule as CSSStyleRule)
    : getFontFaceDeclaration(rule as CSSFontFaceRule);
}

export const stylePropertyValue = captureMethod<CSSStyleDeclaration, [string], string>(
  cssStyleDeclarationPrototype,
  "getPropertyValue",
  (style, name) => style.getPropertyValue(name),
);
export const setStyleProperty = captureMethod<CSSStyleDeclaration, [string, string, string], void>(
  cssStyleDeclarationPrototype,
  "setProperty",
  (style, name, value, priority) => {
    style.setProperty(name, value, priority);
  },
);
export const styleText = captureGetter<CSSStyleDeclaration, string>(
  cssStyleDeclarationPrototype,
  "cssText",
  (style) => style.cssText,
);
const getElementStyle = captureGetter<HTMLElement | SVGElement, CSSStyleDeclaration>(
  htmlElementPrototype,
  "style",
  (element) => element.style,
);
const getSvgElementStyle = captureGetter<SVGElement, CSSStyleDeclaration>(
  typeof SVGElement === "undefined" ? undefined : SVGElement.prototype,
  "style",
  (element) => element.style,
);

export function elementStyle(element: HTMLElement | SVGElement): CSSStyleDeclaration {
  return namespaceUri(element) === "http://www.w3.org/2000/svg"
    ? getSvgElementStyle(element as SVGElement)
    : getElementStyle(element);
}

export const computedStyle = captureMethod<Window, [Element, string | null], CSSStyleDeclaration>(
  windowPropertySource,
  "getComputedStyle",
  (view, element, pseudo) => view.getComputedStyle(element, pseudo),
);

export const elementAnimations = captureMethod<Element, [], Animation[]>(
  elementPrototype,
  "getAnimations",
  (element) => element.getAnimations(),
);
export const elementScrollIntoView = captureMethod<Element, [ScrollIntoViewOptions], void>(
  elementPrototype,
  "scrollIntoView",
  (element, options) => {
    element.scrollIntoView(options);
  },
);
export const pauseAnimation = captureMethod<Animation, [], void>(
  animationPrototype,
  "pause",
  (animation) => {
    animation.pause();
  },
);
export const animationEffect = captureGetter<Animation, AnimationEffect | null>(
  animationPrototype,
  "effect",
  (animation) => animation.effect,
);
export const keyframeTarget = captureGetter<KeyframeEffect, Element | null>(
  keyframeEffectPrototype,
  "target",
  (effect) => effect.target,
);
export const keyframePseudo = captureOptionalGetter<KeyframeEffect, string | null>(
  keyframeEffectPrototype,
  "pseudoElement",
  null,
);
export const keyframes = captureMethod<KeyframeEffect, [], ComputedKeyframe[]>(
  keyframeEffectPrototype,
  "getKeyframes",
  (effect) => effect.getKeyframes(),
);

export function isKeyframeEffect(effect: AnimationEffect | null): effect is KeyframeEffect {
  if (!effect) {
    return false;
  }
  try {
    keyframeTarget(effect as KeyframeEffect);
    return true;
  } catch {
    return false;
  }
}

export const selectionRangeCount = captureGetter<Selection, number>(
  selectionPrototype,
  "rangeCount",
  (selection) => selection.rangeCount,
);
export const selectionRange = captureMethod<Selection, [number], Range>(
  selectionPrototype,
  "getRangeAt",
  (selection, index) => selection.getRangeAt(index),
);
export const rangeCollapsed = captureGetter<Range, boolean>(
  rangePrototype,
  "collapsed",
  (range) => range.collapsed,
);
export const rangeIntersectsNode = captureMethod<Range, [Node], boolean>(
  rangePrototype,
  "intersectsNode",
  (range, node) => range.intersectsNode(node),
);
export const treeWalkerNext = captureMethod<TreeWalker, [], Node | null>(
  treeWalkerPrototype,
  "nextNode",
  (walker) => walker.nextNode(),
);
export const treeWalkerCurrent = captureGetter<TreeWalker, Node>(
  treeWalkerPrototype,
  "currentNode",
  (walker) => walker.currentNode,
);

export const inputType = captureGetter<HTMLInputElement, string>(
  inputPrototype,
  "type",
  (input) => input.type,
);
export const inputValue = captureGetter<HTMLInputElement, string>(
  inputPrototype,
  "value",
  (input) => input.value,
);
export const inputChecked = captureGetter<HTMLInputElement, boolean>(
  inputPrototype,
  "checked",
  (input) => input.checked,
);
export const textAreaValue = captureGetter<HTMLTextAreaElement, string>(
  textAreaPrototype,
  "value",
  (textArea) => textArea.value,
);
export const optionSelected = captureGetter<HTMLOptionElement, boolean>(
  optionPrototype,
  "selected",
  (option) => option.selected,
);
export const detailsOpen = captureGetter<HTMLDetailsElement, boolean>(
  detailsPrototype,
  "open",
  (details) => details.open,
);
export const imageCurrentSource = captureGetter<HTMLImageElement, string>(
  imagePrototype,
  "currentSrc",
  (image) => image.currentSrc,
);
export const setImageSource = captureSetter<HTMLImageElement, string>(
  imagePrototype,
  "src",
  (image, value) => {
    image.src = value;
  },
);
export const setImageWidth = captureSetter<HTMLImageElement, number>(
  imagePrototype,
  "width",
  (image, value) => {
    image.width = value;
  },
);
export const setImageHeight = captureSetter<HTMLImageElement, number>(
  imagePrototype,
  "height",
  (image, value) => {
    image.height = value;
  },
);
export const setImageAlt = captureSetter<HTMLImageElement, string>(
  imagePrototype,
  "alt",
  (image, value) => {
    image.alt = value;
  },
);
export const canvasWidth = captureGetter<HTMLCanvasElement, number>(
  canvasPrototype,
  "width",
  (canvas) => canvas.width,
);
export const canvasHeight = captureGetter<HTMLCanvasElement, number>(
  canvasPrototype,
  "height",
  (canvas) => canvas.height,
);
export const setCanvasWidth = captureSetter<HTMLCanvasElement, number>(
  canvasPrototype,
  "width",
  (canvas, value) => {
    canvas.width = value;
  },
);
export const setCanvasHeight = captureSetter<HTMLCanvasElement, number>(
  canvasPrototype,
  "height",
  (canvas, value) => {
    canvas.height = value;
  },
);
export const canvasDataUrl = captureMethod<HTMLCanvasElement, [string], string>(
  canvasPrototype,
  "toDataURL",
  (canvas, type) => canvas.toDataURL(type),
);
export const canvasContext = captureMethod<
  HTMLCanvasElement,
  ["2d"],
  CanvasRenderingContext2D | null
>(canvasPrototype, "getContext", (canvas, type) => canvas.getContext(type));
export const drawCanvasImage = captureMethod<
  CanvasRenderingContext2D,
  [CanvasImageSource, number, number],
  void
>(context2dPrototype, "drawImage", (context, image, x, y) => {
  context.drawImage(image, x, y);
});
export const mediaCurrentTime = captureGetter<HTMLMediaElement, number>(
  mediaPrototype,
  "currentTime",
  (media) => media.currentTime,
);
export const mediaControls = captureGetter<HTMLMediaElement, boolean>(
  mediaPrototype,
  "controls",
  (media) => media.controls,
);
export const mediaMuted = captureGetter<HTMLMediaElement, boolean>(
  mediaPrototype,
  "muted",
  (media) => media.muted,
);
export const mediaCurrentSource = captureGetter<HTMLMediaElement, string>(
  mediaPrototype,
  "currentSrc",
  (media) => media.currentSrc,
);
export const mediaReadyState = captureGetter<HTMLMediaElement, number>(
  mediaPrototype,
  "readyState",
  (media) => media.readyState,
);
export const videoWidth = captureGetter<HTMLVideoElement, number>(
  videoPrototype,
  "videoWidth",
  (video) => video.videoWidth,
);
export const videoHeight = captureGetter<HTMLVideoElement, number>(
  videoPrototype,
  "videoHeight",
  (video) => video.videoHeight,
);
export const videoPoster = captureGetter<HTMLVideoElement, string>(
  videoPrototype,
  "poster",
  (video) => video.poster,
);
export const elementScrollLeft = captureGetter<Element, number>(
  elementPrototype,
  "scrollLeft",
  (element) => element.scrollLeft,
);
export const elementScrollTop = captureGetter<Element, number>(
  elementPrototype,
  "scrollTop",
  (element) => element.scrollTop,
);
export const elementClientLeft = captureGetter<Element, number>(
  elementPrototype,
  "clientLeft",
  (element) => element.clientLeft,
);
export const elementClientTop = captureGetter<Element, number>(
  elementPrototype,
  "clientTop",
  (element) => element.clientTop,
);
export const elementClientWidth = captureGetter<Element, number>(
  elementPrototype,
  "clientWidth",
  (element) => element.clientWidth,
);
export const elementClientHeight = captureGetter<Element, number>(
  elementPrototype,
  "clientHeight",
  (element) => element.clientHeight,
);
export const elementOffsetWidth = captureGetter<HTMLElement, number>(
  htmlElementPrototype,
  "offsetWidth",
  (element) => element.offsetWidth,
);
export const elementOffsetHeight = captureGetter<HTMLElement, number>(
  htmlElementPrototype,
  "offsetHeight",
  (element) => element.offsetHeight,
);
export const windowScrollX = captureGetter<Window, number>(
  windowPropertySource,
  "scrollX",
  (view) => view.scrollX,
);
export const windowScrollY = captureGetter<Window, number>(
  windowPropertySource,
  "scrollY",
  (view) => view.scrollY,
);
export const windowFrameElement = captureGetter<Window, Element | null>(
  windowPropertySource,
  "frameElement",
  (view) => view.frameElement,
);
export const windowInnerWidth = captureGetter<Window, number>(
  windowPropertySource,
  "innerWidth",
  (view) => view.innerWidth,
);
export const windowInnerHeight = captureGetter<Window, number>(
  windowPropertySource,
  "innerHeight",
  (view) => view.innerHeight,
);
export const windowDeviceScaleFactor = captureGetter<Window, number>(
  windowPropertySource,
  "devicePixelRatio",
  (view) => view.devicePixelRatio,
);
export const locationHref = captureGetter<Location, string>(
  locationPropertySource,
  "href",
  (location) => location.href,
);
export const rectLeft = captureGetter<DOMRectReadOnly, number>(
  domRectPrototype,
  "left",
  (rect) => rect.left,
);
export const rectTop = captureGetter<DOMRectReadOnly, number>(
  domRectPrototype,
  "top",
  (rect) => rect.top,
);
export const rectWidth = captureGetter<DOMRectReadOnly, number>(
  domRectPrototype,
  "width",
  (rect) => rect.width,
);
export const rectHeight = captureGetter<DOMRectReadOnly, number>(
  domRectPrototype,
  "height",
  (rect) => rect.height,
);
