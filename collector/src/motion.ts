import {
  animationMarkerAttribute,
  animationStyleAttribute,
  freezeAttribute,
  freezeCss,
} from "./constants";
import {
  appendChild,
  createElement,
  documentDefaultView,
  isElement,
  namespaceUri,
  ownerDocument,
  querySelector,
  querySelectorAll,
  setAttribute,
  setNodeTextContent,
} from "./dom";
import {
  arrayIncludes,
  arrayJoin,
  arrayPush,
  mapForEach,
  mapGet,
  mapSet,
  objectKeys,
  regExpTest,
  SafeMap,
  SafeSet,
  SafeString,
  SafeTypeError,
  SafeWeakMap,
  SafeWeakSet,
  setAdd,
  setForEach,
  setSize,
  stringReplacePattern,
  stringStartsWith,
  stringToLowerCase,
  weakMapGet,
  weakMapSet,
  weakSetAdd,
  weakSetHas,
} from "./primordials";
import { composedRoots } from "./shadow";
import type { SnapshotContext } from "./types";
import {
  animationEffect,
  computedStyle,
  elementAnimations,
  elementStyle,
  isKeyframeEffect,
  keyframePseudo,
  keyframes,
  keyframeTarget,
  pauseAnimation,
  setStyleProperty,
  stylePropertyValue,
  styleText,
} from "./web";

type CapturedProperties = Map<string, string>;
type CapturedMotion = Map<string, CapturedProperties>;

const frozenDocuments = new SafeWeakSet<Document>();
const failedDocuments = new SafeWeakSet<Document>();
const frozenMotion = new SafeWeakMap<Element, CapturedMotion>();
const generatedMotionStyles = new SafeWeakSet<Element>();

export function freezeMotion(source: Document): void {
  if (weakSetHas(frozenDocuments, source)) {
    return;
  }
  weakSetAdd(frozenDocuments, source);

  try {
    const animations = animationsWithin(source);
    for (let index = 0; index < animations.length; index += 1) {
      try {
        pauseAnimation(animations[index]);
      } catch {
        continue;
      }
    }

    const properties = motionProperties(animations);
    mapForEach(properties, (byPseudo, target) => {
      const captured = captureComputedProperties(target, byPseudo);
      weakMapSet(frozenMotion, target, captured);
      const elementProperties = mapGet(captured, "");
      if (elementProperties && supportsInlineStyle(target)) {
        const style = elementStyle(target as HTMLElement | SVGElement);
        mapForEach(elementProperties, (value, name) => {
          setStyleProperty(style, name, value, "important");
        });
        setStyleProperty(style, "animation", "none", "important");
        setStyleProperty(style, "transition", "none", "important");
      }
    });
  } catch {
    weakSetAdd(failedDocuments, source);
  }
}

export function freezeDocument(source: Document): true {
  freezeMotion(source);
  let style = querySelector(source, `style[${freezeAttribute}]`);
  if (!style) {
    const head = querySelector(source, "head");
    if (!head) {
      throw new SafeTypeError("render freeze requires an HTML head element");
    }
    style = createElement(source, "style");
    setAttribute(style, freezeAttribute, "");
    appendChild(head, style);
  }
  setNodeTextContent(style, freezeCss);
  return true;
}

export function reportMotionCaptureFailure(
  source: Document,
  context: SnapshotContext,
): void {
  if (weakSetHas(failedDocuments, source)) {
    arrayPush(context.warnings, {
      code: "pageknot.animation.capture_failed",
      message: "Animated state could not be frozen at its current phase.",
    });
  }
}

export function materializeMotionState(
  live: Element,
  clone: Element,
  context: SnapshotContext,
  rules: string[],
): void {
  const captured = weakMapGet(frozenMotion, live) ?? captureCurrentMotion(live);
  if (!captured) {
    return;
  }

  let pseudoMarker: string | undefined;
  mapForEach(captured, (properties, pseudo) => {
    if (!pseudo) {
      if (!supportsInlineStyle(clone)) {
        return;
      }
      const style = elementStyle(clone as HTMLElement | SVGElement);
      mapForEach(properties, (value, name) => {
        setStyleProperty(style, name, value, "important");
      });
      setStyleProperty(style, "animation", "none", "important");
      setStyleProperty(style, "transition", "none", "important");
      return;
    }

    const document = ownerDocument(live);
    if (!document) {
      return;
    }
    const declaration = elementStyle(createElement(document, "span"));
    mapForEach(properties, (value, name) => {
      setStyleProperty(declaration, name, value, "important");
    });
    setStyleProperty(declaration, "animation", "none", "important");
    setStyleProperty(declaration, "transition", "none", "important");
    if (pseudoMarker === undefined) {
      pseudoMarker = SafeString(context.nextAnimationMarker);
      context.nextAnimationMarker += 1;
      setAttribute(clone, animationMarkerAttribute, pseudoMarker);
    }
    arrayPush(
      rules,
      `[${animationMarkerAttribute}="${pseudoMarker}"]${pseudo}{${styleText(declaration)}}`,
    );
  });
}

export function appendMotionStyles(
  root: Element | DocumentFragment,
  rules: string[],
): void {
  if (rules.length === 0) {
    return;
  }
  const document = ownerDocument(root);
  if (!document) {
    return;
  }
  const style = createElement(document, "style");
  weakSetAdd(generatedMotionStyles, style);
  setAttribute(style, animationStyleAttribute, "");
  setNodeTextContent(style, arrayJoin(rules, "\n"));
  appendChild(root, style);
}

export function isGeneratedMotionStyle(element: Element): boolean {
  return weakSetHas(generatedMotionStyles, element);
}

function animationsWithin(source: Document): Animation[] {
  const animations = new SafeSet<Animation>();
  const roots = composedRoots(source);
  for (let rootIndex = 0; rootIndex < roots.length; rootIndex += 1) {
    const elements = querySelectorAll(roots[rootIndex], "*");
    for (
      let elementIndex = 0;
      elementIndex < elements.length;
      elementIndex += 1
    ) {
      const observed = elementAnimations(elements[elementIndex]);
      for (
        let animationIndex = 0;
        animationIndex < observed.length;
        animationIndex += 1
      ) {
        setAdd(animations, observed[animationIndex]);
      }
    }
  }
  const result: Animation[] = [];
  setForEach(animations, (animation) => {
    arrayPush(result, animation);
  });
  return result;
}

function motionProperties(
  animations: Animation[],
): Map<Element, Map<string, Set<string>>> {
  const properties = new SafeMap<Element, Map<string, Set<string>>>();
  for (let index = 0; index < animations.length; index += 1) {
    const effect = animationEffect(animations[index]);
    if (!isKeyframeEffect(effect)) {
      continue;
    }
    const target = keyframeTarget(effect);
    if (!target || !isElement(target)) {
      continue;
    }
    const pseudo = keyframePseudo(effect) ?? "";
    if (pseudo && !regExpTest(/^::[a-z-]+$/i, pseudo)) {
      continue;
    }
    const byPseudo =
      mapGet(properties, target) ?? new SafeMap<string, Set<string>>();
    const names = mapGet(byPseudo, pseudo) ?? new SafeSet<string>();
    const frames = keyframes(effect);
    for (let frameIndex = 0; frameIndex < frames.length; frameIndex += 1) {
      const keys = objectKeys(frames[frameIndex]);
      for (let keyIndex = 0; keyIndex < keys.length; keyIndex += 1) {
        const key = keys[keyIndex];
        const name = cssPropertyName(key);
        if (!matchesKeyframeMetadata(key) && name) {
          setAdd(names, name);
        }
      }
    }
    if (setSize(names) > 0) {
      mapSet(byPseudo, pseudo, names);
      mapSet(properties, target, byPseudo);
    }
  }
  return properties;
}

function captureComputedProperties(
  target: Element,
  byPseudo: Map<string, Set<string>>,
): CapturedMotion {
  const captured = new SafeMap<string, CapturedProperties>();
  const document = ownerDocument(target);
  const view = document ? documentDefaultView(document) : null;
  if (!view) {
    return captured;
  }
  mapForEach(byPseudo, (names, pseudo) => {
    const computed = computedStyle(view, target, pseudo || null);
    const values = new SafeMap<string, string>();
    setForEach(names, (name) => {
      mapSet(values, name, stylePropertyValue(computed, name));
    });
    mapSet(captured, pseudo, values);
  });
  return captured;
}

function captureCurrentMotion(live: Element): CapturedMotion | undefined {
  try {
    const properties = motionProperties(elementAnimations(live));
    const byPseudo = mapGet(properties, live);
    return byPseudo ? captureComputedProperties(live, byPseudo) : undefined;
  } catch {
    return undefined;
  }
}

function matchesKeyframeMetadata(name: string): boolean {
  return arrayIncludes(
    ["offset", "computedOffset", "easing", "composite"],
    name,
  );
}

function cssPropertyName(name: string): string {
  if (stringStartsWith(name, "--")) {
    return name;
  }
  const kebab = stringReplacePattern(
    name,
    /[A-Z]/g,
    (letter) => `-${stringToLowerCase(letter)}`,
  );
  return stringStartsWith(kebab, "webkit-") ? `-${kebab}` : kebab;
}

function supportsInlineStyle(
  element: Element,
): element is HTMLElement | SVGElement {
  const namespace = namespaceUri(element);
  return (
    namespace === "http://www.w3.org/1999/xhtml" ||
    namespace === "http://www.w3.org/2000/svg"
  );
}
