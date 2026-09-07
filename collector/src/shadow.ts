import { capturedShadowModeAttribute } from "./constants";
import {
  createElement,
  ownerDocument,
  querySelectorAll,
  setAttribute,
  shadowMode,
  shadowRoot,
} from "./dom";
import {
  arrayPush,
  SafeSet,
  SafeString,
  SafeTypeError,
  SafeWeakMap,
  safeReflectApply,
  setAdd,
  setHas,
  weakMapGet,
  weakMapSet,
} from "./primordials";

const closedRoots = new SafeWeakMap<Element, ShadowRoot>();
const originalAttachShadow = Element.prototype.attachShadow;

Element.prototype.attachShadow = function (this: Element, init: ShadowRootInit): ShadowRoot {
  const normalized: ShadowRootInit = {
    clonable: init.clonable,
    customElementRegistry: init.customElementRegistry,
    delegatesFocus: init.delegatesFocus,
    mode: SafeString(init.mode) as ShadowRootMode,
    serializable: init.serializable,
    slotAssignment: init.slotAssignment,
  };
  const root = safeReflectApply(originalAttachShadow, this, [normalized]) as ShadowRoot;
  if (normalized.mode === "closed") {
    weakMapSet(closedRoots, this, root);
  }
  return root;
};

export function observedShadowRoot(element: Element): ShadowRoot | undefined {
  return shadowRoot(element) ?? weakMapGet(closedRoots, element);
}

export function composedRoots(source: Document | ShadowRoot): Array<Document | ShadowRoot> {
  const roots: Array<Document | ShadowRoot> = [source];
  const visited = new SafeSet<Document | ShadowRoot>();
  setAdd(visited, source);
  for (let index = 0; index < roots.length; index += 1) {
    const elements = querySelectorAll(roots[index], "*");
    for (let elementIndex = 0; elementIndex < elements.length; elementIndex += 1) {
      const element = elements[elementIndex];
      const shadow = observedShadowRoot(element);
      if (shadow && !setHas(visited, shadow)) {
        setAdd(visited, shadow);
        arrayPush(roots, shadow);
      }
    }
  }
  return roots;
}

export function createInspectableShadowTemplate(root: ShadowRoot): HTMLTemplateElement {
  const document = ownerDocument(root);
  if (!document) {
    throw new SafeTypeError("a shadow root has no owner document");
  }
  const template = createElement(document, "template") as HTMLTemplateElement;
  setAttribute(template, "shadowrootmode", "open");
  if (shadowMode(root) === "closed") {
    setAttribute(template, capturedShadowModeAttribute, "closed");
  }
  return template;
}
