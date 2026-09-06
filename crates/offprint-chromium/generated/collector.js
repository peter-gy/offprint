(() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  function __accessProp(key) {
    return this[key];
  }
  var __toCommonJS = (from) => {
    var entry = (__moduleCache ??= new WeakMap).get(from), desc;
    if (entry)
      return entry;
    entry = __defProp({}, "__esModule", { value: true });
    if (from && typeof from === "object" || typeof from === "function") {
      for (var key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(entry, key))
          __defProp(entry, key, {
            get: __accessProp.bind(from, key),
            enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable
          });
    }
    __moduleCache.set(from, entry);
    return entry;
  };
  var __moduleCache;
  var __returnValue = (v) => v;
  function __exportSetter(name, newValue) {
    this[name] = __returnValue.bind(null, newValue);
  }
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, {
        get: all[name],
        enumerable: true,
        configurable: true,
        set: __exportSetter.bind(all, name)
      });
  };

  // src/index.ts
  var exports_src = {};
  __export(exports_src, {
    sha256Fallback: () => sha256Fallback,
    utf8LengthWithinLimit: () => utf8LengthWithinLimit
  });

  // src/identity.ts
  var protocol = { major: 1, minor: 5 };
  var availableCapabilities = [
    "adopted-stylesheets",
    "canvas-pixels",
    "closed-shadow-roots",
    "cssom",
    "form-state",
    "frame-owner-mapping",
    "hidden-element-removal",
    "media-state",
    "open-shadow-roots",
    "responsive-images",
    "selection-capture",
    "selector-capture",
    "unused-css-removal",
    "unused-font-removal"
  ];

  // src/constants.ts
  var manifestElementId = "offprint-manifest";
  var repairDataElementId = "offprint-repair-data";
  var repairScriptElementId = "offprint-repair-script";
  var stateScriptElementId = "offprint-state-script";
  var repairMediaType = "application/vnd.offprint.repair+json";
  var repairMarkerAttribute = "data-offprint-node";
  var documentScrollXAttribute = "data-offprint-scroll-x";
  var documentScrollYAttribute = "data-offprint-scroll-y";
  var elementScrollLeftAttribute = "data-offprint-scroll-left";
  var elementScrollTopAttribute = "data-offprint-scroll-top";
  var animationMarkerAttribute = "data-offprint-animation";
  var animationStyleAttribute = "data-offprint-animation-style";
  var capturedShadowModeAttribute = "data-offprint-shadow-mode";
  var cssBaseAttribute = "data-offprint-css-base";
  var freezeAttribute = "data-offprint-freeze";
  var freezeCss = "*,*::before,*::after{animation-play-state:paused!important;transition:none!important;caret-color:transparent!important}";
  var buildSha256 = "af7ea73dcc333022e9ee5624dc70cd1e9be0811f149fad58c43095c55339a122";

  // src/primordials.ts
  var reflectApply = Reflect.apply;
  var getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
  var getPrototypeOf = Object.getPrototypeOf;
  function uncurryThis(method) {
    return (receiver, ...arguments_) => reflectApply(method, receiver, arguments_);
  }
  function captureGetter(prototype, name, fallback) {
    if (!prototype) {
      return fallback;
    }
    const getter = propertyDescriptor(prototype, name)?.get;
    if (!getter) {
      return () => {
        throw new SafeTypeError(`captured getter is unavailable: ${name}`);
      };
    }
    return (receiver) => reflectApply(getter, receiver, []);
  }
  function captureOptionalGetter(prototype, name, unavailable) {
    const getter = prototype ? propertyDescriptor(prototype, name)?.get : undefined;
    return getter ? (receiver) => reflectApply(getter, receiver, []) : () => unavailable;
  }
  function captureSetter(prototype, name, fallback) {
    if (!prototype) {
      return fallback;
    }
    const setter = propertyDescriptor(prototype, name)?.set;
    if (!setter) {
      return () => {
        throw new SafeTypeError(`captured setter is unavailable: ${name}`);
      };
    }
    return (receiver, value) => {
      reflectApply(setter, receiver, [value]);
    };
  }
  function captureMethod(prototype, name, fallback) {
    if (!prototype) {
      return fallback;
    }
    const method = prototype ? propertyDescriptor(prototype, name)?.value : undefined;
    if (!method) {
      return () => {
        throw new SafeTypeError(`captured method is unavailable: ${name}`);
      };
    }
    return (receiver, ...arguments_) => reflectApply(method, receiver, arguments_);
  }
  function propertyDescriptor(prototype, name) {
    let current = prototype;
    while (current) {
      const descriptor = getOwnPropertyDescriptor(current, name);
      if (descriptor) {
        return descriptor;
      }
      current = getPrototypeOf(current);
    }
    return;
  }
  var SafeMap = Map;
  var SafeSet = Set;
  var SafeWeakMap = WeakMap;
  var SafeWeakSet = WeakSet;
  var SafeUint8Array = Uint8Array;
  var SafeUint32Array = Uint32Array;
  var SafeString = String;
  var SafeNumber = Number;
  var SafeError = Error;
  var SafeRangeError = RangeError;
  var SafeTypeError = TypeError;
  var arrayIsArray = Array.isArray;
  var objectKeys = Object.keys;
  var objectFreeze = Object.freeze;
  var defineProperty = Object.defineProperty;
  var numberIsFinite = Number.isFinite;
  var numberIsSafeInteger = Number.isSafeInteger;
  var mathCeil = Math.ceil;
  var mathFloor = Math.floor;
  var arrayJoin = uncurryThis(Array.prototype.join);
  var arrayPop = uncurryThis(Array.prototype.pop);
  var arrayPush = uncurryThis(Array.prototype.push);
  var arrayIncludes = uncurryThis(Array.prototype.includes);
  var mapGet = uncurryThis(Map.prototype.get);
  var mapSet = uncurryThis(Map.prototype.set);
  var mapDelete = uncurryThis(Map.prototype.delete);
  var mapClear = uncurryThis(Map.prototype.clear);
  var mapForEach = uncurryThis(Map.prototype.forEach);
  var setAdd = uncurryThis(Set.prototype.add);
  var setHas = uncurryThis(Set.prototype.has);
  var setForEach = uncurryThis(Set.prototype.forEach);
  var setSize = captureGetter(Set.prototype, "size", (set) => set.size);
  var weakMapGet = uncurryThis(WeakMap.prototype.get);
  var weakMapSet = uncurryThis(WeakMap.prototype.set);
  var weakSetAdd = uncurryThis(WeakSet.prototype.add);
  var weakSetHas = uncurryThis(WeakSet.prototype.has);
  var stringCharCodeAt = uncurryThis(String.prototype.charCodeAt);
  var stringSlice = uncurryThis(String.prototype.slice);
  var stringStartsWith = uncurryThis(String.prototype.startsWith);
  var stringTrim = uncurryThis(String.prototype.trim);
  var stringToLowerCase = uncurryThis(String.prototype.toLowerCase);
  var stringToLocaleLowerCase = uncurryThis(String.prototype.toLocaleLowerCase);
  var regExpTest = uncurryThis(RegExp.prototype.test);
  var typedArraySubarray = uncurryThis(Uint8Array.prototype.subarray);
  var safeReflectApply = reflectApply;
  var typedArrayByteLength = captureGetter(Uint8Array.prototype, "byteLength", (bytes) => bytes.byteLength);
  var stringReplaceMethod = String.prototype.replace;
  var stringMatchMethod = String.prototype.match;
  function stringReplacePattern(value, search, replacement) {
    return reflectApply(stringReplaceMethod, value, [
      search,
      replacement
    ]);
  }
  function stringMatch(value, pattern) {
    return reflectApply(stringMatchMethod, value, [
      pattern
    ]);
  }
  var textEncoder = new TextEncoder;
  var textEncodeInto = uncurryThis(TextEncoder.prototype.encodeInto);
  function encodeUtf8Chunks(chunks, byteLength) {
    const bytes = new SafeUint8Array(byteLength);
    let offset = 0;
    for (let index = 0;index < chunks.length; index += 1) {
      const destination = typedArraySubarray(bytes, offset);
      const encoded = textEncodeInto(textEncoder, chunks[index], destination);
      if (encoded.read !== chunks[index].length) {
        throw new SafeTypeError("bounded UTF-8 encoding did not consume its input");
      }
      offset += encoded.written;
    }
    return bytes;
  }

  // src/dom.ts
  var nodePrototype = typeof Node === "undefined" ? undefined : Node.prototype;
  var elementPrototype = typeof Element === "undefined" ? undefined : Element.prototype;
  var documentPrototype = typeof Document === "undefined" ? undefined : Document.prototype;
  var fragmentPrototype = typeof DocumentFragment === "undefined" ? undefined : DocumentFragment.prototype;
  var templatePrototype = typeof HTMLTemplateElement === "undefined" ? undefined : HTMLTemplateElement.prototype;
  var framePrototype = typeof HTMLIFrameElement === "undefined" ? undefined : HTMLIFrameElement.prototype;
  var documentTypePrototype = typeof DocumentType === "undefined" ? undefined : DocumentType.prototype;
  var nodeListPrototype = typeof NodeList === "undefined" ? undefined : NodeList.prototype;
  var htmlCollectionPrototype = typeof HTMLCollection === "undefined" ? undefined : HTMLCollection.prototype;
  var namedNodeMapPrototype = typeof NamedNodeMap === "undefined" ? undefined : NamedNodeMap.prototype;
  var attrPrototype = typeof Attr === "undefined" ? undefined : Attr.prototype;
  var shadowRootPrototype = typeof ShadowRoot === "undefined" ? undefined : ShadowRoot.prototype;
  var parserPrototype = typeof DOMParser === "undefined" ? undefined : DOMParser.prototype;
  var SafeDOMParser = typeof DOMParser === "undefined" ? undefined : DOMParser;
  var capturedTreeWalkerNodes = 5;
  var nodeType = captureGetter(nodePrototype, "nodeType", (node) => node.nodeType);
  var firstChild = captureGetter(nodePrototype, "firstChild", (node) => node.firstChild);
  var nextSibling = captureGetter(nodePrototype, "nextSibling", (node) => node.nextSibling);
  var lastChild = captureGetter(nodePrototype, "lastChild", (node) => node.lastChild);
  var previousSibling = captureGetter(nodePrototype, "previousSibling", (node) => node.previousSibling);
  var parentNode = captureGetter(nodePrototype, "parentNode", (node) => node.parentNode);
  var ownerDocument = captureGetter(nodePrototype, "ownerDocument", (node) => node.ownerDocument);
  var nodeValue = captureGetter(nodePrototype, "nodeValue", (node) => node.nodeValue);
  var nodeTextContent = captureGetter(nodePrototype, "textContent", (node) => node.textContent);
  var setNodeTextContent = captureSetter(nodePrototype, "textContent", (node, value) => {
    node.textContent = value;
  });
  var getNodeChildNodes = captureGetter(nodePrototype, "childNodes", (node) => node.childNodes);
  var getNodeBaseUri = captureGetter(nodePrototype, "baseURI", (node) => node.baseURI);
  var getNodeNamespace = captureGetter(elementPrototype, "namespaceURI", (node) => node.namespaceURI);
  var getNodeLocalName = captureGetter(elementPrototype, "localName", (node) => node.localName);
  var cloneNode = captureMethod(nodePrototype, "cloneNode", (node, deep) => node.cloneNode(deep));
  var appendChild = captureMethod(nodePrototype, "appendChild", (node, child) => node.appendChild(child));
  var removeChild = captureMethod(nodePrototype, "removeChild", (node, child) => node.removeChild(child));
  var replaceChild = captureMethod(nodePrototype, "replaceChild", (node, child, replaced) => node.replaceChild(child, replaced));
  var elementQuerySelector = captureMethod(elementPrototype, "querySelector", (element, selector) => element.querySelector(selector));
  var documentQuerySelector = captureMethod(documentPrototype, "querySelector", (document2, selector) => document2.querySelector(selector));
  var fragmentQuerySelector = captureMethod(fragmentPrototype, "querySelector", (fragment, selector) => fragment.querySelector(selector));
  var elementQuerySelectorAll = captureMethod(elementPrototype, "querySelectorAll", (element, selector) => element.querySelectorAll(selector));
  var documentQuerySelectorAll = captureMethod(documentPrototype, "querySelectorAll", (document2, selector) => document2.querySelectorAll(selector));
  var fragmentQuerySelectorAll = captureMethod(fragmentPrototype, "querySelectorAll", (fragment, selector) => fragment.querySelectorAll(selector));
  function querySelector(root, selector) {
    const type = nodeType(root);
    if (type === 9) {
      return documentQuerySelector(root, selector);
    }
    if (type === 11) {
      return fragmentQuerySelector(root, selector);
    }
    return elementQuerySelector(root, selector);
  }
  function querySelectorAll(root, selector) {
    const type = nodeType(root);
    const nodes = type === 9 ? documentQuerySelectorAll(root, selector) : type === 11 ? fragmentQuerySelectorAll(root, selector) : elementQuerySelectorAll(root, selector);
    const result = [];
    const length = nodeListLength(nodes);
    for (let index = 0;index < length; index += 1) {
      const node = nodeListItem(nodes, index);
      if (node) {
        arrayPush(result, node);
      }
    }
    return result;
  }
  var setAttribute = captureMethod(elementPrototype, "setAttribute", (element, name, value) => {
    element.setAttribute(name, value);
  });
  var getAttribute = captureMethod(elementPrototype, "getAttribute", (element, name) => element.getAttribute(name));
  var hasAttribute = captureMethod(elementPrototype, "hasAttribute", (element, name) => element.hasAttribute(name));
  var removeAttribute = captureMethod(elementPrototype, "removeAttribute", (element, name) => {
    element.removeAttribute(name);
  });
  var toggleAttribute = captureMethod(elementPrototype, "toggleAttribute", (element, name, force) => element.toggleAttribute(name, force));
  var contains = captureMethod(nodePrototype, "contains", (node, child) => node.contains(child));
  var elementBounds = captureMethod(elementPrototype, "getBoundingClientRect", (element) => element.getBoundingClientRect());
  var getElementAttributes = captureGetter(elementPrototype, "attributes", (element) => element.attributes);
  var getElementChildren = captureGetter(elementPrototype, "children", (element) => element.children);
  var getElementShadowRoot = captureGetter(elementPrototype, "shadowRoot", (element) => element.shadowRoot);
  function namespaceUri(node) {
    return nodeType(node) === 1 ? getNodeNamespace(node) : null;
  }
  function localName(node) {
    return nodeType(node) === 1 ? getNodeLocalName(node) : null;
  }
  function childNodes(node) {
    const nodes = getNodeChildNodes(node);
    const result = [];
    const length = nodeListLength(nodes);
    for (let index = 0;index < length; index += 1) {
      const child = nodeListItem(nodes, index);
      if (child) {
        arrayPush(result, child);
      }
    }
    return result;
  }
  function elementChildren(element) {
    const children = getElementChildren(element);
    const result = [];
    const length = htmlCollectionLength(children);
    for (let index = 0;index < length; index += 1) {
      const child = htmlCollectionItem(children, index);
      if (child) {
        arrayPush(result, child);
      }
    }
    return result;
  }
  function attributeCount(element) {
    return namedNodeMapLength(getElementAttributes(element));
  }
  function attributeAt(element, index) {
    return namedNodeMapItem(getElementAttributes(element), index);
  }
  function attributeName(attribute) {
    return attrName(attribute);
  }
  function attributeValue(attribute) {
    return attrValue(attribute);
  }
  function shadowRoot(element) {
    return getElementShadowRoot(element);
  }
  var getDocumentElement = captureGetter(documentPrototype, "documentElement", (document2) => document2.documentElement);
  var getDocumentDoctype = captureGetter(documentPrototype, "doctype", (document2) => document2.doctype);
  var getDocumentTitle = captureGetter(documentPrototype, "title", (document2) => document2.title);
  var getDocumentCharacterSet = captureGetter(documentPrototype, "characterSet", (document2) => document2.characterSet);
  var getDocumentDefaultView = captureGetter(documentPrototype, "defaultView", (document2) => document2.defaultView);
  var getDocumentBody = captureGetter(documentPrototype, "body", (document2) => document2.body);
  var getDocumentScrollingElement = captureGetter(documentPrototype, "scrollingElement", (document2) => document2.scrollingElement);
  var getDocumentStyleSheets = captureGetter(documentPrototype, "styleSheets", (document2) => document2.styleSheets);
  var getDocumentAdoptedStyleSheets = captureGetter(documentPrototype, "adoptedStyleSheets", (document2) => document2.adoptedStyleSheets);
  var getFragmentAdoptedStyleSheets = captureGetter(shadowRootPrototype, "adoptedStyleSheets", (fragment) => fragment.adoptedStyleSheets);
  var createElement = captureMethod(documentPrototype, "createElement", (document2, name) => document2.createElement(name));
  var createDocumentFragment = captureMethod(documentPrototype, "createDocumentFragment", (document2) => document2.createDocumentFragment());
  var getSelection = captureMethod(documentPrototype, "getSelection", (document2) => document2.getSelection());
  var createTreeWalker = captureMethod(documentPrototype, "createTreeWalker", (document2, root, show) => document2.createTreeWalker(root, show));
  function documentElement(document2) {
    return getDocumentElement(document2);
  }
  function documentDoctype(document2) {
    return getDocumentDoctype(document2);
  }
  function documentBaseUri(document2) {
    return getNodeBaseUri(document2);
  }
  function documentTitle(document2) {
    return getDocumentTitle(document2);
  }
  function documentCharacterSet(document2) {
    return getDocumentCharacterSet(document2);
  }
  function documentDefaultView(document2) {
    return getDocumentDefaultView(document2);
  }
  function documentBody(document2) {
    return getDocumentBody(document2);
  }
  function documentScrollingElement(document2) {
    return getDocumentScrollingElement(document2);
  }
  function styleSheets(document2) {
    return getDocumentStyleSheets(document2);
  }
  function adoptedStyleSheets(root) {
    return nodeType(root) === 9 ? getDocumentAdoptedStyleSheets(root) : getFragmentAdoptedStyleSheets(root);
  }
  var getTemplateContent = captureGetter(templatePrototype, "content", (template) => template.content);
  var getFrameContentDocument = captureGetter(framePrototype, "contentDocument", (frame) => frame.contentDocument);
  var getFrameContentWindow = captureGetter(framePrototype, "contentWindow", (frame) => frame.contentWindow);
  var getShadowMode = captureGetter(shadowRootPrototype, "mode", (root) => root.mode);
  function templateContent(template) {
    return getTemplateContent(template);
  }
  function frameContentDocument(frame) {
    return getFrameContentDocument(frame);
  }
  function frameContentWindow(frame) {
    return getFrameContentWindow(frame);
  }
  function shadowMode(root) {
    return getShadowMode(root);
  }
  var nodeListLength = captureGetter(nodeListPrototype, "length", (nodes) => nodes.length);
  var nodeListItem = captureMethod(nodeListPrototype, "item", (nodes, index) => nodes.item(index));
  var htmlCollectionLength = captureGetter(htmlCollectionPrototype, "length", (collection) => collection.length);
  var htmlCollectionItem = captureMethod(htmlCollectionPrototype, "item", (collection, index) => collection.item(index));
  var namedNodeMapLength = captureGetter(namedNodeMapPrototype, "length", (attributes) => attributes.length);
  var namedNodeMapItem = captureMethod(namedNodeMapPrototype, "item", (attributes, index) => attributes.item(index));
  var attrName = captureGetter(attrPrototype, "name", (attribute) => attribute.name);
  var attrValue = captureGetter(attrPrototype, "value", (attribute) => attribute.value);
  var getDoctypeName = captureGetter(documentTypePrototype, "name", (doctype) => doctype.name);
  var getDoctypePublicId = captureGetter(documentTypePrototype, "publicId", (doctype) => doctype.publicId);
  var getDoctypeSystemId = captureGetter(documentTypePrototype, "systemId", (doctype) => doctype.systemId);
  function doctypeName(doctype) {
    return getDoctypeName(doctype);
  }
  function doctypePublicId(doctype) {
    return getDoctypePublicId(doctype);
  }
  function doctypeSystemId(doctype) {
    return getDoctypeSystemId(doctype);
  }
  var parseFromString = captureMethod(parserPrototype, "parseFromString", (parser, source, type) => parser.parseFromString(source, type));
  function parseHtml(source) {
    if (!SafeDOMParser) {
      throw new SafeTypeError("DOMParser is unavailable");
    }
    return parseFromString(new SafeDOMParser, source, "text/html");
  }
  function removeNode(node) {
    const parent = parentNode(node);
    if (parent) {
      removeChild(parent, node);
    }
  }
  function replaceNode(node, replacement) {
    const parent = parentNode(node);
    if (parent) {
      replaceChild(parent, replacement, node);
    }
  }
  function isElement(node) {
    return nodeType(node) === 1;
  }

  // src/shadow.ts
  var closedRoots = new SafeWeakMap;
  var originalAttachShadow = Element.prototype.attachShadow;
  Element.prototype.attachShadow = function(init) {
    const normalized = {
      clonable: init.clonable,
      customElementRegistry: init.customElementRegistry,
      delegatesFocus: init.delegatesFocus,
      mode: SafeString(init.mode),
      serializable: init.serializable,
      slotAssignment: init.slotAssignment
    };
    const root = safeReflectApply(originalAttachShadow, this, [
      normalized
    ]);
    if (normalized.mode === "closed") {
      weakMapSet(closedRoots, this, root);
    }
    return root;
  };
  function observedShadowRoot(element) {
    return shadowRoot(element) ?? weakMapGet(closedRoots, element);
  }
  function composedRoots(source) {
    const roots = [source];
    const visited = new SafeSet;
    setAdd(visited, source);
    for (let index = 0;index < roots.length; index += 1) {
      const elements = querySelectorAll(roots[index], "*");
      for (let elementIndex = 0;elementIndex < elements.length; elementIndex += 1) {
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
  function createInspectableShadowTemplate(root) {
    const document2 = ownerDocument(root);
    if (!document2) {
      throw new SafeTypeError("a shadow root has no owner document");
    }
    const template = createElement(document2, "template");
    setAttribute(template, "shadowrootmode", "open");
    if (shadowMode(root) === "closed") {
      setAttribute(template, capturedShadowModeAttribute, "closed");
    }
    return template;
  }

  // src/web.ts
  var styleSheetListPrototype = typeof StyleSheetList === "undefined" ? undefined : StyleSheetList.prototype;
  var cssRuleListPrototype = typeof CSSRuleList === "undefined" ? undefined : CSSRuleList.prototype;
  var cssStyleSheetPrototype = typeof CSSStyleSheet === "undefined" ? undefined : CSSStyleSheet.prototype;
  var styleSheetPrototype = typeof StyleSheet === "undefined" ? undefined : StyleSheet.prototype;
  var cssRulePrototype = typeof CSSRule === "undefined" ? undefined : CSSRule.prototype;
  var cssStyleRulePrototype = typeof CSSStyleRule === "undefined" ? undefined : CSSStyleRule.prototype;
  var cssFontFaceRulePrototype = typeof CSSFontFaceRule === "undefined" ? undefined : CSSFontFaceRule.prototype;
  var cssStyleDeclarationPrototype = typeof CSSStyleDeclaration === "undefined" ? undefined : CSSStyleDeclaration.prototype;
  var windowPrototype = typeof Window === "undefined" ? undefined : Window.prototype;
  var windowObject = typeof window === "undefined" ? undefined : window;
  var windowPropertySource = windowObject ?? windowPrototype;
  var animationPrototype = typeof Animation === "undefined" ? undefined : Animation.prototype;
  var keyframeEffectPrototype = typeof KeyframeEffect === "undefined" ? undefined : KeyframeEffect.prototype;
  var elementPrototype2 = typeof Element === "undefined" ? undefined : Element.prototype;
  var htmlElementPrototype = typeof HTMLElement === "undefined" ? undefined : HTMLElement.prototype;
  var styleElementPrototype = typeof HTMLStyleElement === "undefined" ? undefined : HTMLStyleElement.prototype;
  var svgStyleElementPrototype = typeof SVGStyleElement === "undefined" ? undefined : SVGStyleElement.prototype;
  var linkElementPrototype = typeof HTMLLinkElement === "undefined" ? undefined : HTMLLinkElement.prototype;
  var inputPrototype = typeof HTMLInputElement === "undefined" ? undefined : HTMLInputElement.prototype;
  var textAreaPrototype = typeof HTMLTextAreaElement === "undefined" ? undefined : HTMLTextAreaElement.prototype;
  var optionPrototype = typeof HTMLOptionElement === "undefined" ? undefined : HTMLOptionElement.prototype;
  var detailsPrototype = typeof HTMLDetailsElement === "undefined" ? undefined : HTMLDetailsElement.prototype;
  var imagePrototype = typeof HTMLImageElement === "undefined" ? undefined : HTMLImageElement.prototype;
  var canvasPrototype = typeof HTMLCanvasElement === "undefined" ? undefined : HTMLCanvasElement.prototype;
  var mediaPrototype = typeof HTMLMediaElement === "undefined" ? undefined : HTMLMediaElement.prototype;
  var videoPrototype = typeof HTMLVideoElement === "undefined" ? undefined : HTMLVideoElement.prototype;
  var selectionPrototype = typeof Selection === "undefined" ? undefined : Selection.prototype;
  var rangePrototype = typeof Range === "undefined" ? undefined : Range.prototype;
  var treeWalkerPrototype = typeof TreeWalker === "undefined" ? undefined : TreeWalker.prototype;
  var context2dPrototype = typeof CanvasRenderingContext2D === "undefined" ? undefined : CanvasRenderingContext2D.prototype;
  var domRectPrototype = typeof DOMRectReadOnly === "undefined" ? undefined : DOMRectReadOnly.prototype;
  var locationPrototype = typeof Location === "undefined" ? undefined : Location.prototype;
  var locationObject = typeof location === "undefined" ? undefined : location;
  var locationPropertySource = locationObject ?? locationPrototype;
  var styleSheetListLength = captureGetter(styleSheetListPrototype, "length", (list) => list.length);
  var styleSheetListItem = captureMethod(styleSheetListPrototype, "item", (list, index) => list.item(index));
  var cssRuleListLength = captureGetter(cssRuleListPrototype, "length", (list) => list.length);
  var cssRuleListItem = captureMethod(cssRuleListPrototype, "item", (list, index) => list.item(index));
  var getCssRules = captureGetter(cssStyleSheetPrototype, "cssRules", (sheet) => sheet.cssRules);
  var getStyleSheetOwner = captureGetter(styleSheetPrototype, "ownerNode", (sheet) => sheet.ownerNode);
  var getStyleSheetHref = captureGetter(styleSheetPrototype, "href", (sheet) => sheet.href);
  var getRuleType = captureGetter(cssRulePrototype, "type", (rule) => rule.type);
  var getRuleText = captureGetter(cssRulePrototype, "cssText", (rule) => rule.cssText);
  var getSelectorText = captureGetter(cssStyleRulePrototype, "selectorText", (rule) => rule.selectorText);
  var getStyleRuleDeclaration = captureGetter(cssStyleRulePrototype, "style", (rule) => rule.style);
  var getFontFaceDeclaration = captureGetter(cssFontFaceRulePrototype, "style", (rule) => rule.style);
  var getStyleSheetFromStyle = captureGetter(styleElementPrototype, "sheet", (element) => element.sheet);
  var getStyleSheetFromSvgStyle = captureGetter(svgStyleElementPrototype, "sheet", (element) => element.sheet);
  var getStyleSheetFromLink = captureGetter(linkElementPrototype, "sheet", (element) => element.sheet);
  function styleSheetsFromList(list) {
    const sheets = [];
    const length = styleSheetListLength(list);
    for (let index = 0;index < length; index += 1) {
      const sheet = styleSheetListItem(list, index);
      if (sheet) {
        arrayPush(sheets, sheet);
      }
    }
    return sheets;
  }
  function rulesForStyleSheet(sheet) {
    return getCssRules(sheet);
  }
  function cssRuleCount(list) {
    return cssRuleListLength(list);
  }
  function cssRuleAt(list, index) {
    return cssRuleListItem(list, index);
  }
  function styleSheetOwner(sheet) {
    return getStyleSheetOwner(sheet);
  }
  function styleSheetHref(sheet) {
    return getStyleSheetHref(sheet);
  }
  function documentFontFaces(source) {
    const faces = new SafeSet;
    const sheets = styleSheetsFromList(styleSheets(source));
    for (let sheetIndex = 0;sheetIndex < sheets.length; sheetIndex += 1) {
      const sheet = sheets[sheetIndex];
      if (!styleSheetHref(sheet)) {
        continue;
      }
      try {
        const rules = rulesForStyleSheet(sheet);
        const count = cssRuleCount(rules);
        for (let ruleIndex = 0;ruleIndex < count; ruleIndex += 1) {
          const rule = cssRuleAt(rules, ruleIndex);
          if (rule && cssRuleType(rule) === 5) {
            setAdd(faces, cssRuleText(rule));
          }
        }
      } catch {}
    }
    return faces;
  }
  function styleSheetFor(element) {
    const name = localName(element);
    const namespace = namespaceUri(element);
    if (name === "style") {
      if (namespace === "http://www.w3.org/1999/xhtml") {
        return getStyleSheetFromStyle(element);
      }
      if (namespace === "http://www.w3.org/2000/svg") {
        return getStyleSheetFromSvgStyle(element);
      }
      return null;
    }
    return name === "link" && namespace === "http://www.w3.org/1999/xhtml" ? getStyleSheetFromLink(element) : null;
  }
  function cssRuleType(rule) {
    return getRuleType(rule);
  }
  function cssRuleText(rule) {
    return getRuleText(rule);
  }
  function cssRuleSelector(rule) {
    return getSelectorText(rule);
  }
  function cssRuleDeclaration(rule) {
    return cssRuleType(rule) === 1 ? getStyleRuleDeclaration(rule) : getFontFaceDeclaration(rule);
  }
  var stylePropertyValue = captureMethod(cssStyleDeclarationPrototype, "getPropertyValue", (style, name) => style.getPropertyValue(name));
  var setStyleProperty = captureMethod(cssStyleDeclarationPrototype, "setProperty", (style, name, value, priority) => {
    style.setProperty(name, value, priority);
  });
  var styleText = captureGetter(cssStyleDeclarationPrototype, "cssText", (style) => style.cssText);
  var getElementStyle = captureGetter(htmlElementPrototype, "style", (element) => element.style);
  var getSvgElementStyle = captureGetter(typeof SVGElement === "undefined" ? undefined : SVGElement.prototype, "style", (element) => element.style);
  function elementStyle(element) {
    return namespaceUri(element) === "http://www.w3.org/2000/svg" ? getSvgElementStyle(element) : getElementStyle(element);
  }
  var computedStyle = captureMethod(windowPropertySource, "getComputedStyle", (view, element, pseudo) => view.getComputedStyle(element, pseudo));
  var elementAnimations = captureMethod(elementPrototype2, "getAnimations", (element) => element.getAnimations());
  var elementScrollIntoView = captureMethod(elementPrototype2, "scrollIntoView", (element, options) => {
    element.scrollIntoView(options);
  });
  var pauseAnimation = captureMethod(animationPrototype, "pause", (animation) => {
    animation.pause();
  });
  var animationEffect = captureGetter(animationPrototype, "effect", (animation) => animation.effect);
  var keyframeTarget = captureGetter(keyframeEffectPrototype, "target", (effect) => effect.target);
  var keyframePseudo = captureOptionalGetter(keyframeEffectPrototype, "pseudoElement", null);
  var keyframes = captureMethod(keyframeEffectPrototype, "getKeyframes", (effect) => effect.getKeyframes());
  function isKeyframeEffect(effect) {
    if (!effect) {
      return false;
    }
    try {
      keyframeTarget(effect);
      return true;
    } catch {
      return false;
    }
  }
  var selectionRangeCount = captureGetter(selectionPrototype, "rangeCount", (selection) => selection.rangeCount);
  var selectionRange = captureMethod(selectionPrototype, "getRangeAt", (selection, index) => selection.getRangeAt(index));
  var rangeCollapsed = captureGetter(rangePrototype, "collapsed", (range) => range.collapsed);
  var rangeIntersectsNode = captureMethod(rangePrototype, "intersectsNode", (range, node) => range.intersectsNode(node));
  var treeWalkerNext = captureMethod(treeWalkerPrototype, "nextNode", (walker) => walker.nextNode());
  var treeWalkerCurrent = captureGetter(treeWalkerPrototype, "currentNode", (walker) => walker.currentNode);
  var inputType = captureGetter(inputPrototype, "type", (input) => input.type);
  var inputValue = captureGetter(inputPrototype, "value", (input) => input.value);
  var inputChecked = captureGetter(inputPrototype, "checked", (input) => input.checked);
  var textAreaValue = captureGetter(textAreaPrototype, "value", (textArea) => textArea.value);
  var optionSelected = captureGetter(optionPrototype, "selected", (option) => option.selected);
  var detailsOpen = captureGetter(detailsPrototype, "open", (details) => details.open);
  var imageCurrentSource = captureGetter(imagePrototype, "currentSrc", (image) => image.currentSrc);
  var setImageSource = captureSetter(imagePrototype, "src", (image, value) => {
    image.src = value;
  });
  var setImageWidth = captureSetter(imagePrototype, "width", (image, value) => {
    image.width = value;
  });
  var setImageHeight = captureSetter(imagePrototype, "height", (image, value) => {
    image.height = value;
  });
  var setImageAlt = captureSetter(imagePrototype, "alt", (image, value) => {
    image.alt = value;
  });
  var canvasWidth = captureGetter(canvasPrototype, "width", (canvas) => canvas.width);
  var canvasHeight = captureGetter(canvasPrototype, "height", (canvas) => canvas.height);
  var setCanvasWidth = captureSetter(canvasPrototype, "width", (canvas, value) => {
    canvas.width = value;
  });
  var setCanvasHeight = captureSetter(canvasPrototype, "height", (canvas, value) => {
    canvas.height = value;
  });
  var canvasDataUrl = captureMethod(canvasPrototype, "toDataURL", (canvas, type) => canvas.toDataURL(type));
  var canvasContext = captureMethod(canvasPrototype, "getContext", (canvas, type) => canvas.getContext(type));
  var drawCanvasImage = captureMethod(context2dPrototype, "drawImage", (context, image, x, y) => {
    context.drawImage(image, x, y);
  });
  var mediaCurrentTime = captureGetter(mediaPrototype, "currentTime", (media) => media.currentTime);
  var mediaControls = captureGetter(mediaPrototype, "controls", (media) => media.controls);
  var mediaMuted = captureGetter(mediaPrototype, "muted", (media) => media.muted);
  var mediaCurrentSource = captureGetter(mediaPrototype, "currentSrc", (media) => media.currentSrc);
  var mediaReadyState = captureGetter(mediaPrototype, "readyState", (media) => media.readyState);
  var videoWidth = captureGetter(videoPrototype, "videoWidth", (video) => video.videoWidth);
  var videoHeight = captureGetter(videoPrototype, "videoHeight", (video) => video.videoHeight);
  var videoPoster = captureGetter(videoPrototype, "poster", (video) => video.poster);
  var elementScrollLeft = captureGetter(elementPrototype2, "scrollLeft", (element) => element.scrollLeft);
  var elementScrollTop = captureGetter(elementPrototype2, "scrollTop", (element) => element.scrollTop);
  var elementClientLeft = captureGetter(elementPrototype2, "clientLeft", (element) => element.clientLeft);
  var elementClientTop = captureGetter(elementPrototype2, "clientTop", (element) => element.clientTop);
  var elementClientWidth = captureGetter(elementPrototype2, "clientWidth", (element) => element.clientWidth);
  var elementClientHeight = captureGetter(elementPrototype2, "clientHeight", (element) => element.clientHeight);
  var elementOffsetWidth = captureGetter(htmlElementPrototype, "offsetWidth", (element) => element.offsetWidth);
  var elementOffsetHeight = captureGetter(htmlElementPrototype, "offsetHeight", (element) => element.offsetHeight);
  var windowScrollX = captureGetter(windowPropertySource, "scrollX", (view) => view.scrollX);
  var windowScrollY = captureGetter(windowPropertySource, "scrollY", (view) => view.scrollY);
  var windowFrameElement = captureGetter(windowPropertySource, "frameElement", (view) => view.frameElement);
  var windowInnerWidth = captureGetter(windowPropertySource, "innerWidth", (view) => view.innerWidth);
  var windowInnerHeight = captureGetter(windowPropertySource, "innerHeight", (view) => view.innerHeight);
  var windowDeviceScaleFactor = captureGetter(windowPropertySource, "devicePixelRatio", (view) => view.devicePixelRatio);
  var locationHref = captureGetter(locationPropertySource, "href", (location2) => location2.href);
  var rectLeft = captureGetter(domRectPrototype, "left", (rect) => rect.left);
  var rectTop = captureGetter(domRectPrototype, "top", (rect) => rect.top);
  var rectWidth = captureGetter(domRectPrototype, "width", (rect) => rect.width);
  var rectHeight = captureGetter(domRectPrototype, "height", (rect) => rect.height);

  // src/motion.ts
  var frozenDocuments = new SafeWeakSet;
  var failedDocuments = new SafeWeakSet;
  var frozenMotion = new SafeWeakMap;
  var generatedMotionStyles = new SafeWeakSet;
  function freezeMotion(source) {
    if (weakSetHas(frozenDocuments, source)) {
      return;
    }
    weakSetAdd(frozenDocuments, source);
    try {
      const animations = animationsWithin(source);
      for (let index = 0;index < animations.length; index += 1) {
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
          const style = elementStyle(target);
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
  function freezeDocument(source) {
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
  function reportMotionCaptureFailure(source, context) {
    if (weakSetHas(failedDocuments, source)) {
      arrayPush(context.warnings, {
        code: "offprint.animation.capture_failed",
        message: "Animated state could not be frozen at its current phase."
      });
    }
  }
  function materializeMotionState(live, clone, context, rules) {
    const captured = weakMapGet(frozenMotion, live) ?? captureCurrentMotion(live);
    if (!captured) {
      return;
    }
    let pseudoMarker;
    mapForEach(captured, (properties, pseudo) => {
      if (!pseudo) {
        if (!supportsInlineStyle(clone)) {
          return;
        }
        const style = elementStyle(clone);
        mapForEach(properties, (value, name) => {
          setStyleProperty(style, name, value, "important");
        });
        setStyleProperty(style, "animation", "none", "important");
        setStyleProperty(style, "transition", "none", "important");
        return;
      }
      const document2 = ownerDocument(live);
      if (!document2) {
        return;
      }
      const declaration = elementStyle(createElement(document2, "span"));
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
      arrayPush(rules, `[${animationMarkerAttribute}="${pseudoMarker}"]${pseudo}{${styleText(declaration)}}`);
    });
  }
  function appendMotionStyles(root, rules) {
    if (rules.length === 0) {
      return;
    }
    const document2 = ownerDocument(root);
    if (!document2) {
      return;
    }
    const style = createElement(document2, "style");
    weakSetAdd(generatedMotionStyles, style);
    setAttribute(style, animationStyleAttribute, "");
    setNodeTextContent(style, arrayJoin(rules, `
`));
    appendChild(root, style);
  }
  function isGeneratedMotionStyle(element) {
    return weakSetHas(generatedMotionStyles, element);
  }
  function animationsWithin(source) {
    const animations = new SafeSet;
    const roots = composedRoots(source);
    for (let rootIndex = 0;rootIndex < roots.length; rootIndex += 1) {
      const elements = querySelectorAll(roots[rootIndex], "*");
      for (let elementIndex = 0;elementIndex < elements.length; elementIndex += 1) {
        const observed = elementAnimations(elements[elementIndex]);
        for (let animationIndex = 0;animationIndex < observed.length; animationIndex += 1) {
          setAdd(animations, observed[animationIndex]);
        }
      }
    }
    const result = [];
    setForEach(animations, (animation) => {
      arrayPush(result, animation);
    });
    return result;
  }
  function motionProperties(animations) {
    const properties = new SafeMap;
    for (let index = 0;index < animations.length; index += 1) {
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
      const byPseudo = mapGet(properties, target) ?? new SafeMap;
      const names = mapGet(byPseudo, pseudo) ?? new SafeSet;
      const frames = keyframes(effect);
      for (let frameIndex = 0;frameIndex < frames.length; frameIndex += 1) {
        const keys = objectKeys(frames[frameIndex]);
        for (let keyIndex = 0;keyIndex < keys.length; keyIndex += 1) {
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
  function captureComputedProperties(target, byPseudo) {
    const captured = new SafeMap;
    const document2 = ownerDocument(target);
    const view = document2 ? documentDefaultView(document2) : null;
    if (!view) {
      return captured;
    }
    mapForEach(byPseudo, (names, pseudo) => {
      const computed = computedStyle(view, target, pseudo || null);
      const values = new SafeMap;
      setForEach(names, (name) => {
        mapSet(values, name, stylePropertyValue(computed, name));
      });
      mapSet(captured, pseudo, values);
    });
    return captured;
  }
  function captureCurrentMotion(live) {
    try {
      const properties = motionProperties(elementAnimations(live));
      const byPseudo = mapGet(properties, live);
      return byPseudo ? captureComputedProperties(live, byPseudo) : undefined;
    } catch {
      return;
    }
  }
  function matchesKeyframeMetadata(name) {
    return arrayIncludes(["offset", "computedOffset", "easing", "composite"], name);
  }
  function cssPropertyName(name) {
    if (stringStartsWith(name, "--")) {
      return name;
    }
    const kebab = stringReplacePattern(name, /[A-Z]/g, (letter) => `-${stringToLowerCase(letter)}`);
    return stringStartsWith(kebab, "webkit-") ? `-${kebab}` : kebab;
  }
  function supportsInlineStyle(element) {
    const namespace = namespaceUri(element);
    return namespace === "http://www.w3.org/1999/xhtml" || namespace === "http://www.w3.org/2000/svg";
  }

  // src/protocol.ts
  function utf8LengthWithinLimit(value, maximumBytes) {
    if (!numberIsSafeInteger(maximumBytes) || maximumBytes < 0) {
      throw new SafeTypeError("maximumBytes must be a non-negative safe integer");
    }
    let bytes = 0;
    for (let index = 0;index < value.length; index += 1) {
      const codeUnit = stringCharCodeAt(value, index);
      let width;
      if (codeUnit >= 55296 && codeUnit <= 56319 && index + 1 < value.length) {
        const trailing = stringCharCodeAt(value, index + 1);
        if (trailing >= 56320 && trailing <= 57343) {
          width = 4;
          index += 1;
        } else {
          width = 3;
        }
      } else if (codeUnit <= 127) {
        width = 1;
      } else if (codeUnit <= 2047) {
        width = 2;
      } else {
        width = 3;
      }
      if (bytes > maximumBytes - width) {
        return null;
      }
      bytes += width;
    }
    return bytes;
  }
  function payloadLimitError(captureId, limit, attempted) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.collector.payload_limit",
        message: "collector payload exceeds the configured observation limit",
        details: {
          ...attempted === undefined ? {} : { attempted },
          limit
        }
      }
    };
  }
  function nodeLimitError(captureId, attempted, limit) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.frame.nodes",
        message: "captured frame exceeds the configured DOM node limit",
        details: { attempted, limit }
      }
    };
  }
  function frameLimitError(captureId, attempted, limit) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.frame.limit",
        message: "captured frame graph exceeds the configured frame limit",
        details: { attempted, limit }
      }
    };
  }
  function frameDepthError(captureId, attempted, limit) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.frame.depth",
        message: "captured frame graph exceeds the configured depth",
        details: { attempted, limit }
      }
    };
  }
  function selectorInvalidError(captureId) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.selector.invalid",
        message: "DOM selector is not valid CSS selector syntax"
      }
    };
  }
  function selectorNotFoundError(captureId) {
    return {
      type: "error",
      payload: {
        captureId,
        code: "offprint.selector.not_found",
        message: "top-level document has no element matching the DOM selector"
      }
    };
  }
  var sha256RoundConstants = new SafeUint32Array([
    1116352408,
    1899447441,
    3049323471,
    3921009573,
    961987163,
    1508970993,
    2453635748,
    2870763221,
    3624381080,
    310598401,
    607225278,
    1426881987,
    1925078388,
    2162078206,
    2614888103,
    3248222580,
    3835390401,
    4022224774,
    264347078,
    604807628,
    770255983,
    1249150122,
    1555081692,
    1996064986,
    2554220882,
    2821834349,
    2952996808,
    3210313671,
    3336571891,
    3584528711,
    113926993,
    338241895,
    666307205,
    773529912,
    1294757372,
    1396182291,
    1695183700,
    1986661051,
    2177026350,
    2456956037,
    2730485921,
    2820302411,
    3259730800,
    3345764771,
    3516065817,
    3600352804,
    4094571909,
    275423344,
    430227734,
    506948616,
    659060556,
    883997877,
    958139571,
    1322822218,
    1537002063,
    1747873779,
    1955562222,
    2024104815,
    2227730452,
    2361852424,
    2428436474,
    2756734187,
    3204031479,
    3329325298
  ]);
  function rotateRight(value, bits) {
    return value >>> bits | value << 32 - bits;
  }
  function sha256Fallback(bytes) {
    const words = new SafeUint32Array(64);
    const byteLength = typedArrayByteLength(bytes);
    const paddedBytes = mathCeil((byteLength + 9) / 64) * 64;
    const lengthBytes = new SafeUint8Array(8);
    const bitLengthHigh = mathFloor(byteLength / 536870912);
    const bitLengthLow = byteLength * 8 >>> 0;
    lengthBytes[0] = bitLengthHigh >>> 24;
    lengthBytes[1] = bitLengthHigh >>> 16;
    lengthBytes[2] = bitLengthHigh >>> 8;
    lengthBytes[3] = bitLengthHigh;
    lengthBytes[4] = bitLengthLow >>> 24;
    lengthBytes[5] = bitLengthLow >>> 16;
    lengthBytes[6] = bitLengthLow >>> 8;
    lengthBytes[7] = bitLengthLow;
    let state0 = 1779033703;
    let state1 = 3144134277;
    let state2 = 1013904242;
    let state3 = 2773480762;
    let state4 = 1359893119;
    let state5 = 2600822924;
    let state6 = 528734635;
    let state7 = 1541459225;
    for (let block = 0;block < paddedBytes; block += 64) {
      for (let word = 0;word < 16; word += 1) {
        let value = 0;
        for (let offset = 0;offset < 4; offset += 1) {
          const position = block + word * 4 + offset;
          let byte = 0;
          if (position < byteLength) {
            byte = bytes[position];
          } else if (position === byteLength) {
            byte = 128;
          } else if (position >= paddedBytes - 8) {
            byte = lengthBytes[position - (paddedBytes - 8)];
          }
          value = value << 8 | byte;
        }
        words[word] = value >>> 0;
      }
      for (let word = 16;word < 64; word += 1) {
        const before15 = rotateRight(words[word - 15], 7) ^ rotateRight(words[word - 15], 18) ^ words[word - 15] >>> 3;
        const before2 = rotateRight(words[word - 2], 17) ^ rotateRight(words[word - 2], 19) ^ words[word - 2] >>> 10;
        words[word] = words[word - 16] + before15 + words[word - 7] + before2 >>> 0;
      }
      let a = state0;
      let b = state1;
      let c = state2;
      let d = state3;
      let e = state4;
      let f = state5;
      let g = state6;
      let h = state7;
      for (let round = 0;round < 64; round += 1) {
        const upper = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
        const choice = e & f ^ ~e & g;
        const first = h + upper + choice + sha256RoundConstants[round] + words[round] >>> 0;
        const lower = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
        const majority = a & b ^ a & c ^ b & c;
        const second = lower + majority >>> 0;
        h = g;
        g = f;
        f = e;
        e = d + first >>> 0;
        d = c;
        c = b;
        b = a;
        a = first + second >>> 0;
      }
      state0 = state0 + a >>> 0;
      state1 = state1 + b >>> 0;
      state2 = state2 + c >>> 0;
      state3 = state3 + d >>> 0;
      state4 = state4 + e >>> 0;
      state5 = state5 + f >>> 0;
      state6 = state6 + g >>> 0;
      state7 = state7 + h >>> 0;
    }
    let digest = "";
    const states = [
      state0,
      state1,
      state2,
      state3,
      state4,
      state5,
      state6,
      state7
    ];
    for (let index = 0;index < states.length; index += 1) {
      digest += hexadecimal(states[index], 8);
    }
    return digest;
  }
  async function sha256(bytes) {
    return sha256Fallback(bytes);
  }
  function crc32(bytes) {
    let crc = 4294967295;
    const byteLength = typedArrayByteLength(bytes);
    for (let index = 0;index < byteLength; index += 1) {
      const byte = bytes[index];
      crc ^= byte;
      for (let bit = 0;bit < 8; bit += 1) {
        crc = crc >>> 1 ^ 3988292384 & -(crc & 1);
      }
    }
    return (crc ^ 4294967295) >>> 0;
  }
  function hexadecimal(value, width) {
    const digits = "0123456789abcdef";
    let encoded = "";
    let remaining = value >>> 0;
    for (let index = 0;index < width; index += 1) {
      encoded = digits[remaining & 15] + encoded;
      remaining >>>= 4;
    }
    return encoded;
  }

  // src/serialize.ts
  class BoundedWriter {
    maximumBytes;
    bytes = 0;
    chunks = [];
    overflow = false;
    constructor(maximumBytes) {
      this.maximumBytes = maximumBytes;
    }
    write(value) {
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
    resultString() {
      if (this.overflow) {
        return { attempted: this.maximumBytes + 1, kind: "limit" };
      }
      return {
        bytes: this.bytes,
        kind: "ok",
        value: arrayJoin(this.chunks, "")
      };
    }
    resultBytes() {
      if (this.overflow) {
        return { attempted: this.maximumBytes + 1, kind: "limit" };
      }
      return {
        bytes: this.bytes,
        kind: "ok",
        value: encodeUtf8Chunks(this.chunks, this.bytes)
      };
    }
  }
  function safeStringSet(values) {
    const result = new SafeSet;
    for (let index = 0;index < values.length; index += 1) {
      setAdd(result, values[index]);
    }
    return result;
  }
  var voidElements = safeStringSet([
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
    "wbr"
  ]);
  var rawTextElements = safeStringSet([
    "script",
    "style",
    "xmp",
    "iframe",
    "noembed",
    "noframes"
  ]);
  function serializeHtmlBounded(root, maximumBytes) {
    const writer = new BoundedWriter(maximumBytes);
    const pending = [{ kind: "node", node: root, rawText: false }];
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
        writeEscaped(writer, nodeValue(node) ?? "", task.rawText ? "raw" : "text");
        continue;
      }
      if (type === 8) {
        if (!writer.write("<!--") || !writer.write(nodeValue(node) ?? "") || !writer.write("-->")) {
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
      const element = node;
      const name = localName(element) ?? "";
      if (!writer.write(`<${name}`)) {
        break;
      }
      const count = attributeCount(element);
      for (let index = 0;index < count; index += 1) {
        const attribute = attributeAt(element, index);
        if (!attribute) {
          continue;
        }
        if (!writer.write(` ${attributeName(attribute)}="`) || !writeEscaped(writer, attributeValue(attribute), "attribute") || !writer.write('"')) {
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
      const childRoot = isHtml && name === "template" ? templateContent(element) : element;
      for (let child = lastChild(childRoot);child; child = previousSibling(child)) {
        arrayPush(pending, { kind: "node", node: child, rawText });
      }
    }
    return writer.resultString();
  }
  function serializeJsonBytesBounded(value, maximumBytes) {
    const writer = new BoundedWriter(maximumBytes);
    writeJson(writer, value);
    return writer.resultBytes();
  }
  function serializeJsonStringBounded(value, maximumBytes) {
    const writer = new BoundedWriter(maximumBytes);
    writeJson(writer, value);
    return writer.resultString();
  }
  function escapeScriptDataBounded(value, maximumBytes) {
    const writer = new BoundedWriter(maximumBytes);
    let start = 0;
    for (let index = 0;index < value.length; index += 1) {
      const code = stringCharCodeAt(value, index);
      const replacement = code === 38 ? "\\u0026" : code === 60 ? "\\u003c" : code === 62 ? "\\u003e" : code === 8232 ? "\\u2028" : code === 8233 ? "\\u2029" : undefined;
      if (replacement === undefined) {
        continue;
      }
      if (!writer.write(stringSlice(value, start, index)) || !writer.write(replacement)) {
        return writer.resultString();
      }
      start = index + 1;
    }
    writer.write(stringSlice(value, start));
    return writer.resultString();
  }
  function writeJson(writer, value) {
    const pending = [{ kind: "value", value }];
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
        if (!writer.write('"') || !writeEscaped(writer, task.value, "json") || !writer.write('"')) {
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
        for (let index = current.length - 1;index >= 0; index -= 1) {
          arrayPush(pending, { kind: "value", value: current[index] });
          if (index > 0) {
            arrayPush(pending, { kind: "literal", value: "," });
          }
        }
        arrayPush(pending, { kind: "literal", value: "[" });
      } else if (typeof current === "object") {
        const keys = objectKeys(current);
        arrayPush(pending, { kind: "literal", value: "}" });
        for (let index = keys.length - 1;index >= 0; index -= 1) {
          const key = keys[index];
          arrayPush(pending, {
            kind: "value",
            value: current[key]
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
  function writeEscaped(writer, value, mode) {
    let start = 0;
    for (let index = 0;index < value.length; index += 1) {
      const code = stringCharCodeAt(value, index);
      const replacement = mode === "json" ? jsonEscape(code) : mode === "raw" ? undefined : htmlEscape(code, mode === "attribute");
      if (replacement === undefined) {
        continue;
      }
      if (!writer.write(stringSlice(value, start, index)) || !writer.write(replacement)) {
        return false;
      }
      start = index + 1;
    }
    return writer.write(stringSlice(value, start));
  }
  function htmlEscape(code, attribute) {
    if (code === 38) {
      return "&amp;";
    }
    if (code === 60) {
      return "&lt;";
    }
    if (attribute && code === 34) {
      return "&quot;";
    }
    return;
  }
  function jsonEscape(code) {
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
        return "\\\"";
      case 92:
        return "\\\\";
      default:
        return code < 32 ? `\\u00${"0123456789abcdef"[code >>> 4 & 15]}${"0123456789abcdef"[code & 15]}` : undefined;
    }
  }

  // src/repair.ts
  var htmlNamespace = "http://www.w3.org/1999/xhtml";
  function removeReservedMetadata(root) {
    const remove = [];
    walkElements(root, (element) => {
      if (arrayIncludes([
        manifestElementId,
        repairDataElementId,
        repairScriptElementId,
        stateScriptElementId
      ], getAttribute(element, "id") ?? "") || hasAttribute(element, animationStyleAttribute) && !isGeneratedMotionStyle(element)) {
        arrayPush(remove, element);
      }
    });
    for (let index = 0;index < remove.length; index += 1) {
      removeNode(remove[index]);
    }
  }
  function serializeDocumentWithRepair(source, root, maximumBytes) {
    assignRepairMarkers(root);
    const initial = serializeHtmlBounded(root, maximumBytes);
    if (initial.kind === "limit") {
      return initial;
    }
    const structuralRepair = structuralRepairFor(root, initial.value);
    if (structuralRepair) {
      const repairJson = serializeJsonStringBounded(structuralRepair, maximumBytes);
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
  function structuralRepairFor(root, serialized) {
    const reparsed = parseHtml(serialized);
    if (repairNodesEqual(root, documentElement(reparsed))) {
      return;
    }
    return { documentElement: repairNode(root) };
  }
  function repairNodesEqual(left, right) {
    const pending = [[left, right]];
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
      if (getAttribute(leftNode, repairMarkerAttribute) !== getAttribute(rightNode, repairMarkerAttribute) || (namespaceUri(leftNode) ?? htmlNamespace) !== (namespaceUri(rightNode) ?? htmlNamespace) || (localName(leftNode) ?? "") !== (localName(rightNode) ?? "") || shadowModeFor(leftNode) !== shadowModeFor(rightNode)) {
        return false;
      }
      const leftChildren = childNodes(leftNode);
      const rightChildren = childNodes(rightNode);
      if (leftChildren.length !== rightChildren.length) {
        return false;
      }
      for (let index = 0;index < leftChildren.length; index += 1) {
        arrayPush(pending, [leftChildren[index], rightChildren[index]]);
      }
      const leftTemplate = templateChildren(leftNode);
      const rightTemplate = templateChildren(rightNode);
      if (leftTemplate.length !== rightTemplate.length) {
        return false;
      }
      for (let index = 0;index < leftTemplate.length; index += 1) {
        arrayPush(pending, [leftTemplate[index], rightTemplate[index]]);
      }
    }
    return true;
  }
  function repairNode(node) {
    if (nodeType(node) === 3) {
      return { kind: "text", value: nodeValue(node) ?? "" };
    }
    if (nodeType(node) === 8) {
      return { kind: "comment", value: nodeValue(node) ?? "" };
    }
    if (!isElement(node)) {
      throw new SafeTypeError("structural repair supports element, text, and comment nodes");
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
      ...shadowMode ? { shadowMode } : {}
    };
  }
  function repairChildren(parent) {
    return repairNodes(childNodes(parent));
  }
  function repairNodes(nodes) {
    const repaired = [];
    for (let index = 0;index < nodes.length; index += 1) {
      arrayPush(repaired, repairNode(nodes[index]));
    }
    return repaired;
  }
  function templateChildren(node) {
    return isHtmlTemplate(node) ? childNodes(templateContent(node)) : [];
  }
  function shadowModeFor(node) {
    return isHtmlTemplate(node) ? getAttribute(node, "shadowrootmode") ?? undefined : undefined;
  }
  function isHtmlTemplate(node) {
    return namespaceUri(node) === htmlNamespace && localName(node) === "template";
  }
  function assignRepairMarkers(root) {
    let nextMarker = 0;
    walkElements(root, (element) => {
      setAttribute(element, repairMarkerAttribute, SafeString(nextMarker));
      nextMarker += 1;
    });
  }
  function removeRepairMarkers(root) {
    walkElements(root, (element) => {
      removeAttribute(element, repairMarkerAttribute);
    });
  }
  function walkElements(root, visit) {
    const pending = [root];
    while (pending.length > 0) {
      const element = arrayPop(pending);
      if (!element) {
        continue;
      }
      visit(element);
      const children = elementChildren(element);
      const template = templateChildren(element);
      for (let index = 0;index < template.length; index += 1) {
        const child = template[index];
        if (isElement(child)) {
          arrayPush(children, child);
        }
      }
      for (let index = children.length - 1;index >= 0; index -= 1) {
        arrayPush(pending, children[index]);
      }
    }
  }
  function appendRepairData(source, root, repairData) {
    const head = querySelector(root, "head");
    if (!head) {
      throw new SafeTypeError("structural repair requires an HTML head element");
    }
    const data = createElement(source, "script");
    setAttribute(data, "id", repairDataElementId);
    setAttribute(data, "type", repairMediaType);
    setNodeTextContent(data, repairData);
    appendChild(head, data);
  }

  // src/scope.ts
  function resolveSelector(source, options) {
    if (options.selector === undefined) {
      return {};
    }
    let target;
    try {
      target = querySelector(source, options.selector);
    } catch {
      return { error: selectorInvalidError(options.captureId) };
    }
    return target ? { target } : { error: selectorNotFoundError(options.captureId) };
  }
  function snapshotOptionsFor(options) {
    return {
      captureScope: options.captureScope,
      preservePasswordValues: options.preservePasswordValues,
      removeHiddenElements: options.removeHiddenElements,
      removeUnusedCss: options.removeUnusedCss,
      removeUnusedFonts: options.removeUnusedFonts
    };
  }
  function retainCloneBranch(target, boundary) {
    let retained = target;
    while (retained !== boundary) {
      const parent = parentNode(retained);
      if (!parent) {
        throw new SafeTypeError("selector target clone is detached");
      }
      const siblings = childNodes(parent);
      for (let index = 0;index < siblings.length; index += 1) {
        if (siblings[index] !== retained) {
          removeNode(siblings[index]);
        }
      }
      retained = parent;
    }
  }
  function removeCloneChildren(node) {
    const children = childNodes(node);
    for (let index = 0;index < children.length; index += 1) {
      removeNode(children[index]);
    }
  }
  function applySelectorScope(source, target, cloneRoot, clones) {
    const targetClone = mapGet(clones, target);
    if (!targetClone || !contains(cloneRoot, targetClone)) {
      throw new SafeTypeError("selector target clone is unavailable");
    }
    const sourceRoot = documentElement(source);
    if (target === sourceRoot) {
      return;
    }
    const body = documentBody(source);
    if (body && contains(body, target)) {
      const bodyClone = mapGet(clones, body);
      if (!bodyClone) {
        throw new SafeTypeError("document body clone is unavailable");
      }
      retainCloneBranch(targetClone, bodyClone);
      return;
    }
    const head = querySelector(source, "head");
    if (head && contains(head, target)) {
      const headClone = mapGet(clones, head);
      if (!headClone) {
        throw new SafeTypeError("document head clone is unavailable");
      }
      retainCloneBranch(targetClone, headClone);
      if (body) {
        const bodyClone = mapGet(clones, body);
        if (bodyClone) {
          removeCloneChildren(bodyClone);
        }
      }
      return;
    }
    retainCloneBranch(targetClone, cloneRoot);
  }

  // src/state.ts
  function copyState(live, clone, context, snapshotInline) {
    if (isHtmlElement(live, "input") && isHtmlElement(clone, "input")) {
      const liveInput = live;
      const cloneInput = clone;
      if (inputType(liveInput) === "password" && !context.options.preservePasswordValues) {
        removeAttribute(cloneInput, "value");
        arrayPush(context.warnings, {
          code: "offprint.form.password_redacted",
          message: "A password value was redacted."
        });
      } else {
        setAttribute(cloneInput, "value", inputValue(liveInput));
      }
      toggleAttribute(cloneInput, "checked", inputChecked(liveInput));
    } else if (isHtmlElement(live, "textarea") && isHtmlElement(clone, "textarea")) {
      setNodeTextContent(clone, textAreaValue(live));
    } else if (isHtmlElement(live, "option") && isHtmlElement(clone, "option")) {
      toggleAttribute(clone, "selected", optionSelected(live));
    } else if (isHtmlElement(live, "details") && isHtmlElement(clone, "details")) {
      toggleAttribute(clone, "open", detailsOpen(live));
    } else if (isHtmlElement(live, "img") && isHtmlElement(clone, "img")) {
      const liveImage = live;
      const currentSource = imageCurrentSource(liveImage);
      if (currentSource) {
        setAttribute(clone, "src", currentSource);
        removeAttribute(clone, "srcset");
        removeAttribute(clone, "sizes");
      }
    } else if (isHtmlElement(live, "canvas") && isHtmlElement(clone, "canvas")) {
      const liveCanvas = live;
      const cloneCanvas = clone;
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
          context.budget.settlePayloadAllocation(context.reservation, allocatedPayloadBytes, 0);
        }
        if (context.budget.limitResponse(error)) {
          throw error;
        }
        materializeVisualFallback(liveCanvas, cloneCanvas, "canvas", context);
      }
    } else if (isHtmlElement(live, "video", "audio") && isHtmlElement(clone, "video", "audio")) {
      const liveMedia = live;
      const cloneMedia = clone;
      setAttribute(cloneMedia, "data-offprint-current-time", SafeString(mediaCurrentTime(liveMedia)));
      toggleAttribute(cloneMedia, "controls", mediaControls(liveMedia));
      toggleAttribute(cloneMedia, "muted", mediaMuted(liveMedia));
      const currentSource = mediaCurrentSource(liveMedia);
      if (currentSource) {
        setAttribute(cloneMedia, "src", currentSource);
      }
      if (isHtmlElement(live, "video") && isHtmlElement(clone, "video")) {
        materializeVideo(live, clone, context);
      }
    } else if (isHtmlElement(live, "iframe") && isHtmlElement(clone, "iframe")) {
      copyInlineFrame(live, clone, context, snapshotInline);
    }
  }
  function prepareElementState(live, clone) {
    const reserved = [
      documentScrollXAttribute,
      documentScrollYAttribute,
      elementScrollLeftAttribute,
      elementScrollTopAttribute,
      animationMarkerAttribute
    ];
    for (let index = 0;index < reserved.length; index += 1) {
      removeAttribute(clone, reserved[index]);
    }
    const document2 = ownerDocument(live);
    if (document2 && documentScrollingElement(document2) === live) {
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
  function copyInlineFrame(liveFrame, clone, context, snapshotInline) {
    try {
      const childDocument = frameContentDocument(liveFrame);
      if (!childDocument || !documentElement(childDocument)) {
        return;
      }
      const childWindow = frameContentWindow(liveFrame);
      const childCollector = childWindow?.__offprintCollector;
      const options = { ...context.options, captureScope: "page" };
      const visualFallbackIdPrefix = `${context.visualFallbackIdPrefix}inline-${SafeString(context.nextInlineFallbackNamespace)}-`;
      context.nextInlineFallbackNamespace += 1;
      const request = {
        budget: context.budget,
        frameDepth: context.frameDepth + 1,
        options,
        visualFallbackIdPrefix
      };
      const child = childCollector ? childCollector.call("snapshotInline", [request]) : snapshotInline(childDocument, request);
      context.budget.recordNestedSnapshot(context.reservation, child.payloadBytes, child.subtreeNodes, child.frames);
      mapSet(context.inlineFrameOwners, clone, child.frameOwners);
      setAttribute(clone, "srcdoc", mergeInlineVisualFallbacks(liveFrame, child, context));
      setAttribute(clone, "data-offprint-frame-base", documentBaseUri(childDocument));
      removeAttribute(clone, "src");
      for (let index = 0;index < child.warnings.length; index += 1) {
        arrayPush(context.warnings, child.warnings[index]);
      }
    } catch (error) {
      if (context.budget.limitResponse(error)) {
        throw error;
      }
      arrayPush(context.warnings, {
        code: "offprint.frame.cross_origin",
        message: "A frame requires collection through its attached target."
      });
    }
  }
  function mergeInlineVisualFallbacks(frame, child, context) {
    const bounds = elementBounds(frame);
    const document2 = ownerDocument(frame);
    const parentWindow = document2 ? documentDefaultView(document2) : null;
    const childWindow = frameContentWindow(frame);
    const offsetWidth = elementOffsetWidth(frame);
    const offsetHeight = elementOffsetHeight(frame);
    const width = rectWidth(bounds);
    const height = rectHeight(bounds);
    const scaleX = offsetWidth > 0 ? width / offsetWidth : 1;
    const scaleY = offsetHeight > 0 ? height / offsetHeight : 1;
    const contentX = rectLeft(bounds) + (parentWindow ? windowScrollX(parentWindow) : 0) + elementClientLeft(frame) * scaleX;
    const contentY = rectTop(bounds) + (parentWindow ? windowScrollY(parentWindow) : 0) + elementClientTop(frame) * scaleY;
    for (let index = 0;index < child.visualFallbacks.length; index += 1) {
      const fallback = child.visualFallbacks[index];
      arrayPush(context.visualFallbacks, {
        ...fallback,
        x: SafeString(contentX + (SafeNumber(fallback.x) - (childWindow ? windowScrollX(childWindow) : 0)) * scaleX),
        y: SafeString(contentY + (SafeNumber(fallback.y) - (childWindow ? windowScrollY(childWindow) : 0)) * scaleY),
        width: SafeString(SafeNumber(fallback.width) * scaleX),
        height: SafeString(SafeNumber(fallback.height) * scaleY)
      });
      const target = mapGet(child.visualFallbackTargets, fallback.id);
      if (target) {
        mapSet(context.visualFallbackTargets, fallback.id, target);
      }
    }
    return child.html;
  }
  function isHtmlElement(node, ...localNames) {
    return isElement(node) && namespaceUri(node) === "http://www.w3.org/1999/xhtml" && arrayIncludes(localNames, localName(node) ?? "");
  }
  function canvasImage(live, clone) {
    const document2 = ownerDocument(live);
    if (!document2) {
      throw new SafeTypeError("a canvas has no owner document");
    }
    const image = createElement(document2, "img");
    copyAttributes(clone, image);
    setImageWidth(image, canvasWidth(live));
    setImageHeight(image, canvasHeight(live));
    setImageAlt(image, getAttribute(clone, "aria-label") ?? "");
    return image;
  }
  function videoImage(live, clone) {
    const document2 = ownerDocument(live);
    if (!document2) {
      throw new SafeTypeError("a video has no owner document");
    }
    const image = createElement(document2, "img");
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
  function copyAttributes(source, destination) {
    const count = attributeCount(source);
    for (let index = 0;index < count; index += 1) {
      const attribute = attributeAt(source, index);
      if (attribute) {
        setAttribute(destination, attributeName(attribute), attributeValue(attribute));
      }
    }
  }
  function materializeVideo(live, clone, context) {
    const readyState = mediaReadyState(live);
    const width = videoWidth(live);
    const height = videoHeight(live);
    if (readyState >= 2 && width > 0 && height > 0) {
      let reservedPayloadBytes = 0;
      let allocatedPayloadBytes = 0;
      try {
        reservedPayloadBytes = reserveCanvasDataUrl(width, height, context);
        const document2 = ownerDocument(live);
        if (!document2) {
          throw new SafeTypeError("a video has no owner document");
        }
        const canvas = createElement(document2, "canvas");
        setCanvasWidth(canvas, width);
        setCanvasHeight(canvas, height);
        const drawing = canvasContext(canvas, "2d");
        if (!drawing) {
          throw new SafeError("2D canvas context is unavailable");
        }
        drawCanvasImage(drawing, live, 0, 0);
        const encoded = encodeReservedCanvasDataUrl(canvas, reservedPayloadBytes, context);
        reservedPayloadBytes = 0;
        allocatedPayloadBytes = encoded.bytes;
        const image = videoImage(live, clone);
        setImageSource(image, encoded.value);
        setAttribute(image, "data-offprint-media-frame", "");
        replaceNode(clone, image);
        return;
      } catch (error) {
        if (reservedPayloadBytes > 0) {
          context.budget.settlePayloadAllocation(context.reservation, reservedPayloadBytes, 0);
        }
        if (allocatedPayloadBytes > 0) {
          context.budget.settlePayloadAllocation(context.reservation, allocatedPayloadBytes, 0);
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
  function captureCanvasDataUrl(canvas, context) {
    const reservedBytes = reserveCanvasDataUrl(canvasWidth(canvas), canvasHeight(canvas), context);
    try {
      return encodeReservedCanvasDataUrl(canvas, reservedBytes, context);
    } catch (error) {
      context.budget.settlePayloadAllocation(context.reservation, reservedBytes, 0);
      throw error;
    }
  }
  function reserveCanvasDataUrl(width, height, context) {
    const maximumBytes = maximumPngDataUrlBytes(width, height);
    const availableBytes = context.budget.maximumPayloadAllocation(context.reservation);
    if (maximumBytes === null || maximumBytes > availableBytes) {
      context.budget.rejectPayloadAllocation(context.reservation, maximumBytes ?? availableBytes + 1);
    }
    context.budget.reservePayloadAllocation(context.reservation, maximumBytes);
    return maximumBytes;
  }
  function encodeReservedCanvasDataUrl(canvas, reservedBytes, context) {
    const value = canvasDataUrl(canvas, "image/png");
    const actualBytes = utf8LengthWithinLimit(value, reservedBytes);
    if (actualBytes === null) {
      context.budget.rejectPayloadAllocation(context.reservation, reservedBytes + 1);
    }
    context.budget.settlePayloadAllocation(context.reservation, reservedBytes, actualBytes);
    return { bytes: actualBytes, value };
  }
  function maximumPngDataUrlBytes(width, height) {
    if (!numberIsSafeInteger(width) || width < 0 || !numberIsSafeInteger(height) || height < 0) {
      return null;
    }
    const rowBytes = width * 4 + 1;
    const sourceBytes = rowBytes * height;
    if (!numberIsSafeInteger(rowBytes) || !numberIsSafeInteger(sourceBytes)) {
      return null;
    }
    const deflateBytes = sourceBytes + mathCeil(sourceBytes / 4096) + mathCeil(sourceBytes / 16384) + mathCeil(sourceBytes / 33554432) + 13;
    const chunkBytes = mathCeil(deflateBytes / 65535) * 12;
    const pngBytes = deflateBytes + chunkBytes + 4096;
    const encodedBytes = 22 + mathCeil(pngBytes / 3) * 4;
    return numberIsSafeInteger(encodedBytes) ? encodedBytes : null;
  }
  function materializeVisualFallback(live, clone, kind, context) {
    if (!context.allowScreenshotFallback) {
      arrayPush(context.warnings, {
        code: `offprint.${kind}.capture_unavailable`,
        message: `The ${kind} bitmap could not be collected from an inline frame.`
      });
      return;
    }
    const bounds = elementBounds(live);
    if (!numberIsFinite(rectLeft(bounds)) || !numberIsFinite(rectTop(bounds)) || !numberIsFinite(rectWidth(bounds)) || !numberIsFinite(rectHeight(bounds)) || rectWidth(bounds) <= 0 || rectHeight(bounds) <= 0) {
      arrayPush(context.warnings, {
        code: `offprint.${kind}.empty_bounds`,
        message: `The ${kind} bitmap has no visible capture bounds.`
      });
      return;
    }
    const id = context.visualFallbackIdPrefix + SafeString(context.visualFallbacks.length);
    const image = isHtmlElement(live, "canvas") && isHtmlElement(clone, "canvas") ? canvasImage(live, clone) : videoImage(live, clone);
    setAttribute(image, "data-offprint-visual-fallback", id);
    setAttribute(image, `data-offprint-${kind}`, "");
    replaceNode(clone, image);
    mapSet(context.visualFallbackTargets, id, live);
    const document2 = ownerDocument(live);
    const view = document2 ? documentDefaultView(document2) : null;
    arrayPush(context.visualFallbacks, {
      id,
      kind,
      x: SafeString(rectLeft(bounds) + (view ? windowScrollX(view) : 0)),
      y: SafeString(rectTop(bounds) + (view ? windowScrollY(view) : 0)),
      width: SafeString(rectWidth(bounds)),
      height: SafeString(rectHeight(bounds))
    });
  }
  function positionVisualFallback(target) {
    elementScrollIntoView(target, {
      block: "center",
      inline: "center"
    });
    let document2 = ownerDocument(target);
    let view = document2 ? documentDefaultView(document2) : null;
    let frame = view ? windowFrameElement(view) : null;
    while (frame) {
      elementScrollIntoView(frame, {
        block: "center",
        inline: "center"
      });
      document2 = ownerDocument(frame);
      view = document2 ? documentDefaultView(document2) : null;
      frame = view ? windowFrameElement(view) : null;
    }
    let bounds = elementBounds(target);
    let x = rectLeft(bounds);
    let y = rectTop(bounds);
    let width = rectWidth(bounds);
    let height = rectHeight(bounds);
    document2 = ownerDocument(target);
    view = document2 ? documentDefaultView(document2) : null;
    frame = view ? windowFrameElement(view) : null;
    while (frame) {
      bounds = elementBounds(frame);
      const offsetWidth = elementOffsetWidth(frame);
      const offsetHeight = elementOffsetHeight(frame);
      const scaleX = offsetWidth > 0 ? rectWidth(bounds) / offsetWidth : 1;
      const scaleY = offsetHeight > 0 ? rectHeight(bounds) / offsetHeight : 1;
      x = rectLeft(bounds) + elementClientLeft(frame) * scaleX + x * scaleX;
      y = rectTop(bounds) + elementClientTop(frame) * scaleY + y * scaleY;
      width *= scaleX;
      height *= scaleY;
      document2 = ownerDocument(frame);
      view = document2 ? documentDefaultView(document2) : null;
      frame = view ? windowFrameElement(view) : null;
    }
    return {
      x: SafeString(x + (view ? windowScrollX(view) : 0)),
      y: SafeString(y + (view ? windowScrollY(view) : 0)),
      width: SafeString(width),
      height: SafeString(height)
    };
  }

  // src/styles.ts
  function copyCssRules(sheet, root, context, maximumBytes) {
    try {
      const usedFonts = context.options.removeUnusedFonts ? usedFontsForRoot(root, context) : undefined;
      const inheritedFontFaces = nodeType(root) === 11 && styleSheetOwner(sheet) === null ? context.documentFontFaces : undefined;
      const copied = [];
      let bytes = 0;
      const rules = rulesForStyleSheet(sheet);
      const count = cssRuleCount(rules);
      for (let index = 0;index < count; index += 1) {
        if (bytes >= maximumBytes) {
          return { attempted: maximumBytes + 1, kind: "limit" };
        }
        const rule = cssRuleAt(rules, index);
        if (rule && keepCssRule(rule, root, context, usedFonts, inheritedFontFaces)) {
          const separatorBytes = copied.length > 0 ? 1 : 0;
          if (bytes > maximumBytes - separatorBytes) {
            return { attempted: maximumBytes + 1, kind: "limit" };
          }
          const remainingBytes = maximumBytes - bytes - separatorBytes;
          if (remainingBytes < 1) {
            return { attempted: maximumBytes + 1, kind: "limit" };
          }
          const text = cssRuleText(rule);
          const width = utf8LengthWithinLimit(text, remainingBytes);
          if (width === null) {
            return { attempted: maximumBytes + 1, kind: "limit" };
          }
          bytes += separatorBytes + width;
          arrayPush(copied, text);
        }
      }
      return { bytes, kind: "ok", rules: copied };
    } catch {
      arrayPush(context.warnings, {
        code: "offprint.cssom.unreadable",
        message: "A stylesheet could not be read through the CSS Object Model."
      });
      return;
    }
  }
  function materializeCssRules(sheet, root, context, allocatePayload) {
    const unclaimedPayloadBytes = context.budget.maximumPayloadAllocation(context.reservation);
    const maximumBytes = allocatePayload ? unclaimedPayloadBytes : context.budget.maximumDocumentBytes(context.reservation);
    context.budget.reservePayloadAllocation(context.reservation, unclaimedPayloadBytes);
    const copied = copyCssRules(sheet, root, context, maximumBytes);
    if (!copied) {
      context.budget.settlePayloadAllocation(context.reservation, unclaimedPayloadBytes, 0);
      return;
    }
    if (copied.kind === "limit") {
      context.budget.settlePayloadAllocation(context.reservation, unclaimedPayloadBytes, 0);
      if (allocatePayload) {
        context.budget.rejectPayloadAllocation(context.reservation, copied.attempted);
      }
      context.budget.rejectPayload(copied.attempted);
    }
    context.budget.settlePayloadAllocation(context.reservation, unclaimedPayloadBytes, allocatePayload ? copied.bytes : 0);
    return arrayJoin(copied.rules, `
`);
  }
  var statefulSelector = /::|:(?:active|any-link|autofill|checked|defined|disabled|enabled|focus|focus-visible|focus-within|fullscreen|future|has|host|hover|indeterminate|link|modal|open|past|paused|picture-in-picture|placeholder-shown|playing|read-only|read-write|required|target|user-invalid|user-valid|valid|visited)\b/i;
  var pseudoElements = ["::before", "::after"];
  function keepCssRule(rule, root, context, usedFonts, inheritedFontFaces) {
    const kind = cssRuleType(rule);
    if (context.options.removeUnusedCss && kind === 1) {
      const selector = cssRuleSelector(rule);
      if (!regExpTest(statefulSelector, selector)) {
        try {
          if (!querySelector(root, selector)) {
            return false;
          }
        } catch {
          return true;
        }
      }
    }
    if (kind === 5 && inheritedFontFaces && setHas(inheritedFontFaces, cssRuleText(rule))) {
      return false;
    }
    if (usedFonts && kind === 5 && !hasUsedFont(fontFamilies(stylePropertyValue(cssRuleDeclaration(rule), "font-family")), usedFonts)) {
      return false;
    }
    return true;
  }
  function usedFontsForRoot(root, context) {
    const cached = mapGet(context.usedFontsByRoot, root);
    if (cached) {
      return cached;
    }
    const families = usedFontFamilies(root);
    mapSet(context.usedFontsByRoot, root, families);
    return families;
  }
  function usedFontFamilies(root) {
    const families = new SafeSet;
    const elements = querySelectorAll(root, "*");
    for (let index = 0;index < elements.length; index += 1) {
      const element = elements[index];
      const document2 = ownerDocument(element);
      const view = document2 ? documentDefaultView(document2) : null;
      if (!view) {
        continue;
      }
      const style = computedStyle(view, element, null);
      const elementFonts = fontFamilies(stylePropertyValue(style, "font-family"));
      for (let familyIndex = 0;familyIndex < elementFonts.length; familyIndex += 1) {
        setAdd(families, elementFonts[familyIndex]);
      }
      for (let pseudoIndex = 0;pseudoIndex < pseudoElements.length; pseudoIndex += 1) {
        const pseudoStyle = computedStyle(view, element, pseudoElements[pseudoIndex]);
        if (stylePropertyValue(pseudoStyle, "content") !== "none") {
          const pseudoFonts = fontFamilies(stylePropertyValue(pseudoStyle, "font-family"));
          for (let familyIndex = 0;familyIndex < pseudoFonts.length; familyIndex += 1) {
            setAdd(families, pseudoFonts[familyIndex]);
          }
        }
      }
    }
    return families;
  }
  function fontFamilies(value) {
    const matches = stringMatch(value, /"[^"]*"|'[^']*'|[^,]+/g) ?? [];
    const families = [];
    for (let index = 0;index < matches.length; index += 1) {
      const family = stringToLocaleLowerCase(stringReplacePattern(stringTrim(matches[index]), /^(['"])(.*)\1$/, "$2"));
      if (family) {
        arrayPush(families, family);
      }
    }
    return families;
  }
  function hasUsedFont(families, usedFonts) {
    for (let index = 0;index < families.length; index += 1) {
      if (setHas(usedFonts, families[index])) {
        return true;
      }
    }
    return false;
  }
  function appendAdoptedStyles(root, cloneRoot, context) {
    const sheets = adoptedStyleSheets(root);
    for (let index = 0;index < sheets.length; index += 1) {
      const sheet = sheets[index];
      const css = materializeCssRules(sheet, root, context, true);
      if (css === undefined) {
        continue;
      }
      const style = createElement(documentFor(root), "style");
      setAttribute(style, "data-offprint-adopted", "");
      markStyleBase(style, sheet);
      setNodeTextContent(style, css);
      appendChild(cloneRoot, style);
    }
  }
  function applyCssom(source, clones, context) {
    const sheets = nodeType(source) === 9 ? styleSheetsFromList(styleSheets(source)) : ownedStyleSheets(source);
    for (let index = 0;index < sheets.length; index += 1) {
      const sheet = sheets[index];
      const owner = styleSheetOwner(sheet);
      if (!owner || nodeType(owner) !== 1) {
        continue;
      }
      const ownerClone = mapGet(clones, owner);
      if (!ownerClone || !isElement(ownerClone)) {
        continue;
      }
      const replacesStyleText = namespaceUri(ownerClone) === "http://www.w3.org/1999/xhtml" && localName(ownerClone) === "style";
      const css = materializeCssRules(sheet, source, context, !replacesStyleText);
      if (css === undefined) {
        continue;
      }
      if (replacesStyleText) {
        setNodeTextContent(ownerClone, css);
        markStyleBase(ownerClone, sheet);
      } else {
        const style = createElement(documentFor(source), "style");
        setAttribute(style, "data-offprint-cssom", "");
        markStyleBase(style, sheet);
        setNodeTextContent(style, css);
        replaceNode(ownerClone, style);
      }
    }
  }
  function markStyleBase(element, sheet) {
    const href = styleSheetHref(sheet);
    if (href) {
      setAttribute(element, cssBaseAttribute, href);
    }
  }
  function ownedStyleSheets(root) {
    const sheets = [];
    const owners = querySelectorAll(root, 'style, link[rel~="stylesheet"]');
    for (let index = 0;index < owners.length; index += 1) {
      const sheet = styleSheetFor(owners[index]);
      if (sheet) {
        arrayPush(sheets, sheet);
      }
    }
    return sheets;
  }
  function documentFor(root) {
    if (nodeType(root) === 9) {
      return root;
    }
    const document2 = ownerDocument(root);
    if (!document2) {
      throw new SafeTypeError("a shadow root has no owner document");
    }
    return document2;
  }

  // src/collection.ts
  function copyShadowRoot(liveRoot, cloneHost, context) {
    const template = createInspectableShadowTemplate(liveRoot);
    const document2 = ownerDocument(liveRoot);
    if (!document2) {
      throw new SafeTypeError("a shadow root has no owner document");
    }
    const fragment = createDocumentFragment(document2);
    const liveChildren = childNodes(liveRoot);
    for (let index = 0;index < liveChildren.length; index += 1) {
      appendChild(fragment, cloneNode(liveChildren[index], true));
    }
    const motionRules = [];
    const clones = materializePairs(liveRoot, fragment, context, motionRules);
    applyCssom(liveRoot, clones, context);
    appendAdoptedStyles(liveRoot, fragment, context);
    appendMotionStyles(fragment, motionRules);
    appendChild(templateContent(template), fragment);
    appendChild(cloneHost, template);
  }
  function materializePairs(liveRoot, cloneRoot, context, motionRules) {
    const stack = [[liveRoot, cloneRoot]];
    const pairs = [];
    const clones = new SafeMap;
    while (stack.length > 0) {
      const pair = arrayPop(stack);
      if (!pair) {
        break;
      }
      arrayPush(pairs, pair);
      const live = pair[0];
      const clone = pair[1];
      const liveChildren = childNodes(live);
      const cloneChildren = childNodes(clone);
      const count = liveChildren.length < cloneChildren.length ? liveChildren.length : cloneChildren.length;
      for (let index = count - 1;index >= 0; index -= 1) {
        arrayPush(stack, [liveChildren[index], cloneChildren[index]]);
      }
    }
    const snapshotInline = snapshotInlineDocument;
    for (let pairIndex = 0;pairIndex < pairs.length; pairIndex += 1) {
      const live = pairs[pairIndex][0];
      const clone = pairs[pairIndex][1];
      mapSet(clones, live, clone);
      if (isElement(live) && isElement(clone)) {
        removeAttribute(clone, cssBaseAttribute);
        prepareElementState(live, clone);
        materializeMotionState(live, clone, context, motionRules);
      }
      copyState(live, clone, context, snapshotInline);
      if (isElement(live) && isElement(clone)) {
        const liveElement = live;
        const shadow = observedShadowRoot(liveElement);
        if (shadow) {
          copyShadowRoot(shadow, clone, context);
        }
        if (context.options.removeHiddenElements && isLayoutlessHiddenElement(liveElement)) {
          removeNode(clone);
        }
      }
    }
    return clones;
  }
  function isLayoutlessHiddenElement(element) {
    const document2 = ownerDocument(element);
    const view = document2 ? documentDefaultView(document2) : null;
    const body = document2 ? documentBody(document2) : null;
    return (body ? contains(body, element) : false) && (view ? stylePropertyValue(computedStyle(view, element, null), "display") : "") === "none";
  }
  function applySelectionScope(source, clones) {
    const selection = getSelection(source);
    const ranges = [];
    if (selection) {
      const rangeCount = selectionRangeCount(selection);
      for (let index = 0;index < rangeCount; index += 1) {
        const range = selectionRange(selection, index);
        if (!rangeCollapsed(range)) {
          arrayPush(ranges, range);
        }
      }
    }
    const body = documentBody(source);
    if (!body || ranges.length === 0) {
      return { ranges: 0, nodes: 0 };
    }
    const selected = new SafeSet;
    setAdd(selected, body);
    const nodes = [];
    const walker = createTreeWalker(source, body, capturedTreeWalkerNodes);
    while (treeWalkerNext(walker)) {
      arrayPush(nodes, treeWalkerCurrent(walker));
    }
    for (let rangeIndex = 0;rangeIndex < ranges.length; rangeIndex += 1) {
      const range = ranges[rangeIndex];
      for (let nodeIndex = 0;nodeIndex < nodes.length; nodeIndex += 1) {
        const node = nodes[nodeIndex];
        try {
          if (!rangeIntersectsNode(range, node)) {
            continue;
          }
        } catch {
          continue;
        }
        for (let current = node;current && current !== body; current = parentNode(current)) {
          setAdd(selected, current);
        }
      }
    }
    for (let index = 0;index < nodes.length; index += 1) {
      const node = nodes[index];
      if (!setHas(selected, node) && parentNode(node) && setHas(selected, parentNode(node))) {
        const clone = mapGet(clones, node);
        if (clone) {
          removeNode(clone);
        }
      }
    }
    const cloneBody = mapGet(clones, body);
    if (cloneBody && isElement(cloneBody)) {
      setAttribute(cloneBody, "data-offprint-selection", "");
    }
    return {
      ranges: ranges.length,
      nodes: 1 + (cloneBody && isElement(cloneBody) ? querySelectorAll(cloneBody, "*").length : 0)
    };
  }
  function snapshotDocument(source, rootContext) {
    const reservation = rootContext.budget.reserveDocument(source, rootContext.frameDepth);
    const context = {
      ...rootContext,
      documentFontFaces: documentFontFaces(source),
      inlineFrameOwners: new SafeMap,
      reservation,
      usedFontsByRoot: new SafeMap
    };
    freezeMotion(source);
    reportMotionCaptureFailure(source, context);
    const liveDocumentElement = documentElement(source);
    const clone = cloneNode(liveDocumentElement, true);
    const motionRules = [];
    const clones = materializePairs(liveDocumentElement, clone, context, motionRules);
    const view = documentDefaultView(source);
    setAttribute(clone, documentScrollXAttribute, SafeString(view ? windowScrollX(view) : 0));
    setAttribute(clone, documentScrollYAttribute, SafeString(view ? windowScrollY(view) : 0));
    const liveFrameOwners = querySelectorAll(source, "iframe, frame");
    const removableSources = querySelectorAll(clone, "picture source, video source, audio source");
    for (let index = 0;index < removableSources.length; index += 1) {
      removeNode(removableSources[index]);
    }
    applyCssom(source, clones, context);
    const head = querySelector(clone, "head");
    if (head) {
      appendMotionStyles(head, motionRules);
    }
    const selection = context.options.captureScope === "selection" ? applySelectionScope(source, clones) : { ranges: 0, nodes: 0 };
    if (rootContext.selectorTarget) {
      applySelectorScope(source, rootContext.selectorTarget, clone, clones);
    }
    const adoptedStyleHost = querySelector(clone, "body") ?? head;
    if (adoptedStyleHost) {
      appendAdoptedStyles(source, adoptedStyleHost, context);
    }
    removeReservedMetadata(clone);
    const frameOwners = [];
    let retainedIndex = 0;
    for (let originalIndex = 0;originalIndex < liveFrameOwners.length; originalIndex += 1) {
      const liveOwner = liveFrameOwners[originalIndex];
      const cloneOwner = mapGet(clones, liveOwner);
      if (cloneOwner && isElement(cloneOwner) && contains(clone, cloneOwner)) {
        const mappings = frameOwnerMappings(originalIndex, retainedIndex, mapGet(context.inlineFrameOwners, cloneOwner) ?? []);
        for (let index = 0;index < mappings.length; index += 1) {
          arrayPush(frameOwners, mappings[index]);
        }
        retainedIndex += 1;
      }
    }
    const maximumDocumentBytes = context.budget.maximumDocumentBytes(reservation);
    const serialized = serializeDocumentWithRepair(source, clone, maximumDocumentBytes);
    if (serialized.kind === "limit") {
      context.budget.rejectPayload(serialized.attempted);
    }
    context.budget.commitDocument(reservation, serialized.bytes);
    return {
      frames: reservation.nestedFrames + 1,
      html: serialized.value,
      nodes: reservation.nodes,
      payloadBytes: serialized.bytes,
      selection,
      subtreeNodes: reservation.nodes + reservation.nestedNodes,
      frameOwners
    };
  }
  function frameOwnerMappings(originalIndex, retainedIndex, nested) {
    const mappings = [
      {
        originalPath: [originalIndex],
        retainedPath: [retainedIndex]
      }
    ];
    for (let index = 0;index < nested.length; index += 1) {
      const originalPath = [originalIndex];
      const retainedPath = [retainedIndex];
      const nestedMapping = nested[index];
      for (let pathIndex = 0;pathIndex < nestedMapping.originalPath.length; pathIndex += 1) {
        arrayPush(originalPath, nestedMapping.originalPath[pathIndex]);
      }
      for (let pathIndex = 0;pathIndex < nestedMapping.retainedPath.length; pathIndex += 1) {
        arrayPush(retainedPath, nestedMapping.retainedPath[pathIndex]);
      }
      arrayPush(mappings, { originalPath, retainedPath });
    }
    return mappings;
  }
  function snapshotInlineDocument(source, request) {
    const warnings = [];
    const visualFallbacks = [];
    const visualFallbackTargets = new SafeMap;
    const snapshot = snapshotDocument(source, {
      warnings,
      visualFallbacks,
      visualFallbackTargets,
      visualFallbackIdPrefix: request.visualFallbackIdPrefix,
      allowScreenshotFallback: true,
      budget: request.budget,
      frameDepth: request.frameDepth,
      nextAnimationMarker: 0,
      nextInlineFallbackNamespace: 0,
      options: request.options
    });
    return { ...snapshot, warnings, visualFallbacks, visualFallbackTargets };
  }
  function doctypeText(source) {
    const doctype = documentDoctype(source);
    if (!doctype) {
      return "<!doctype html>";
    }
    const publicIdentifier = doctypePublicId(doctype);
    const systemIdentifier = doctypeSystemId(doctype);
    const publicId = publicIdentifier ? ` PUBLIC "${publicIdentifier}"` : "";
    const systemId = systemIdentifier ? `${publicIdentifier ? "" : " SYSTEM"} "${systemIdentifier}"` : "";
    return `<!DOCTYPE ${doctypeName(doctype)}${publicId}${systemId}>`;
  }

  // src/budget.ts
  var htmlNamespace2 = "http://www.w3.org/1999/xhtml";
  var elementNode = 1;
  var documentNode = 9;
  var documentFragmentNode = 11;

  class SnapshotLimitError extends Error {
    response;
    constructor(response) {
      super(response.payload.message);
      this.name = "OffprintSnapshotLimitError";
      this.response = response;
      weakSetAdd(snapshotLimitErrors, this);
    }
  }
  var snapshotLimitErrors = new SafeWeakSet;
  function snapshotLimitResponse(error) {
    if (typeof error !== "object" || error === null || !weakSetHas(snapshotLimitErrors, error)) {
      return;
    }
    return error.response;
  }

  class RecursiveSnapshotBudget {
    captureId;
    maximumFrameDepth;
    maximumFrames;
    maximumPayloadBytes;
    consumedFrames = 0;
    remainingFrames;
    remainingNodes;
    remainingPayloadBytes;
    constructor(captureId, maximumNodes, maximumPayloadBytes, maximumFrames, maximumFrameDepth) {
      this.captureId = captureId;
      this.maximumFrameDepth = maximumFrameDepth;
      this.maximumFrames = maximumFrames;
      this.maximumPayloadBytes = maximumPayloadBytes;
      this.remainingFrames = maximumFrames;
      this.remainingNodes = maximumNodes;
      this.remainingPayloadBytes = maximumPayloadBytes;
    }
    reserveDocument(source, frameDepth) {
      if (frameDepth > this.maximumFrameDepth) {
        throw new SnapshotLimitError(frameDepthError(this.captureId, frameDepth, this.maximumFrameDepth));
      }
      if (this.remainingFrames < 1) {
        throw new SnapshotLimitError(frameLimitError(this.captureId, this.consumedFrames + 1, this.maximumFrames));
      }
      const measured = preflightDocumentWithinLimits(source, this.remainingNodes, this.remainingPayloadBytes);
      if (measured.kind !== "ok") {
        if (measured.kind === "nodes") {
          throw new SnapshotLimitError(nodeLimitError(this.captureId, measured.attempted, this.remainingNodes));
        }
        throw new SnapshotLimitError(payloadLimitError(this.captureId, this.maximumPayloadBytes, this.maximumPayloadBytes - this.remainingPayloadBytes + measured.attempted));
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
        sourcePayloadBytes: measured.payloadBytes
      };
    }
    recordNestedSnapshot(reservation, payloadBytes, nodes, frames) {
      reservation.nestedPayloadBytes += payloadBytes;
      reservation.nestedNodes += nodes;
      reservation.nestedFrames += frames;
    }
    maximumDocumentBytes(reservation) {
      return reservation.allocatedPayloadBytes + reservation.nestedPayloadBytes + reservation.sourcePayloadBytes + this.remainingPayloadBytes;
    }
    maximumPayloadAllocation(_reservation) {
      return this.remainingPayloadBytes;
    }
    reservePayloadAllocation(reservation, payloadBytes) {
      if (!numberIsSafeInteger(payloadBytes) || payloadBytes < 0) {
        throw new SafeTypeError("payloadBytes must be a non-negative safe integer");
      }
      if (payloadBytes > this.remainingPayloadBytes) {
        throw new SnapshotLimitError(payloadLimitError(this.captureId, this.maximumPayloadBytes, this.maximumPayloadBytes - this.remainingPayloadBytes + payloadBytes));
      }
      this.remainingPayloadBytes -= payloadBytes;
      reservation.allocatedPayloadBytes += payloadBytes;
    }
    settlePayloadAllocation(reservation, reservedBytes, actualBytes) {
      if (!numberIsSafeInteger(reservedBytes) || reservedBytes < 0 || !numberIsSafeInteger(actualBytes) || actualBytes < 0 || reservedBytes > reservation.allocatedPayloadBytes) {
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
    limitResponse(error) {
      return snapshotLimitResponse(error);
    }
    commitDocument(reservation, payloadBytes) {
      const documentPayload = payloadBytes - reservation.nestedPayloadBytes;
      const ownPayloadBytes = documentPayload > 0 ? documentPayload : 0;
      const adjustment = ownPayloadBytes - reservation.sourcePayloadBytes;
      const availablePayloadBytes = this.remainingPayloadBytes + reservation.allocatedPayloadBytes;
      if (adjustment > availablePayloadBytes) {
        throw new SnapshotLimitError(payloadLimitError(this.captureId, this.maximumPayloadBytes, this.maximumPayloadBytes - availablePayloadBytes + adjustment));
      }
      this.remainingPayloadBytes = availablePayloadBytes - adjustment;
      reservation.allocatedPayloadBytes = 0;
    }
    rejectPayload(attempted) {
      throw new SnapshotLimitError(payloadLimitError(this.captureId, this.maximumPayloadBytes, attempted));
    }
    rejectPayloadAllocation(_reservation, attemptedBytes) {
      throw new SnapshotLimitError(payloadLimitError(this.captureId, this.maximumPayloadBytes, this.maximumPayloadBytes - this.remainingPayloadBytes + attemptedBytes));
    }
  }
  function preflightDocumentWithinLimits(source, maximumNodes, maximumPayloadBytes) {
    let nodes = 0;
    let payloadBytes = 0;
    const pending = [source];
    while (pending.length > 0) {
      const node = arrayPop(pending);
      if (!node) {
        continue;
      }
      if (nodeType(node) !== documentNode && nodeType(node) !== documentFragmentNode) {
        nodes += 1;
        if (nodes > maximumNodes) {
          return { attempted: nodes, kind: "nodes" };
        }
        const measured = measureDomNodePayload(node, maximumPayloadBytes - payloadBytes);
        if (measured === null) {
          return {
            attempted: maximumPayloadBytes + 1,
            kind: "payload"
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
      if (namespaceUri(node) === htmlNamespace2 && localName(node) === "template") {
        arrayPush(pending, templateContent(node));
      }
      const shadow = observedShadowRoot(node);
      if (shadow) {
        arrayPush(pending, shadow);
      }
    }
    return { kind: "ok", nodes, payloadBytes };
  }
  function measureDomNodePayload(node, maximumBytes) {
    let bytes = 0;
    const add = (value, fixedBytes = 0) => {
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
      const element = node;
      const name = localName(element) ?? "";
      if (!add(name, 2)) {
        return null;
      }
      const count = attributeCount(element);
      for (let index = 0;index < count; index += 1) {
        const attribute = attributeAt(element, index);
        if (attribute && (!add(attributeName(attribute), 1) || !add(attributeValue(attribute), 3))) {
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
      return add(doctypeName(node), 11) ? bytes : null;
    }
    return add(nodeValue(node) ?? "") ? bytes : null;
  }

  // src/dispatch.ts
  function dispatchCollector(collector, method, arguments_) {
    switch (method) {
      case "freeze":
        return collector.freeze();
      case "handshake":
        return collector.handshake(arguments_[0], arguments_[1], arguments_[2], arguments_[3]);
      case "prepare":
        return collector.prepare(arguments_[0]);
      case "positionVisualFallback":
        return collector.positionVisualFallback(arguments_[0]);
      case "describe":
        return collector.describe(arguments_[0], arguments_[1]);
      case "read":
        return collector.read(arguments_[0], arguments_[1], arguments_[2]);
      case "acknowledge":
        return collector.acknowledge(arguments_[0], arguments_[1], arguments_[2]);
      case "release":
        return collector.release(arguments_[0], arguments_[1]);
      case "snapshotInline":
        return collector.snapshotInline(arguments_[0]);
      default:
        throw new SafeTypeError("collector method is unavailable");
    }
  }

  // src/visual-fallback.ts
  var targets = new SafeMap;
  var fallbackTargets = objectFreeze({
    map: targets,
    clear() {
      mapClear(targets);
    },
    position(id) {
      const target = mapGet(targets, id);
      if (!target) {
        throw new SafeTypeError("visual fallback target is unavailable");
      }
      return positionVisualFallback(target);
    }
  });

  // src/index.ts
  var observations = new SafeMap;
  function key(captureId, frameId) {
    return `${captureId}:${frameId}`;
  }
  var collector = objectFreeze({
    call(method, arguments_) {
      return dispatchCollector(collector, method, arguments_);
    },
    handshake(captureId, hostBuildSha256, requestedCapabilities, maximumChunkBytes) {
      return {
        protocol,
        captureId,
        hostBuildSha256,
        collectorBuildSha256: buildSha256,
        requestedCapabilities,
        availableCapabilities,
        maximumChunkBytes
      };
    },
    freeze() {
      return freezeDocument(document);
    },
    async prepare(options) {
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
      if (!numberIsSafeInteger(options.frameDepth) || options.frameDepth < 0 || !numberIsSafeInteger(options.maximumFrameDepth) || options.maximumFrameDepth < 0) {
        return frameDepthError(options.captureId, options.frameDepth, options.maximumFrameDepth);
      }
      const budget = new RecursiveSnapshotBudget(options.captureId, options.maximumNodes, options.maximumPayloadBytes, options.maximumFrames, options.maximumFrameDepth);
      const warnings = [];
      const visualFallbacks = [];
      const selector = resolveSelector(document, options);
      if (selector.error)
        return selector.error;
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
          selectorTarget: selector.target
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
          scrollY: SafeString(windowScrollY(window))
        },
        frames: snapshot.frames,
        nodes: snapshot.nodes,
        subtreeNodes: snapshot.subtreeNodes,
        warnings,
        visualFallbacks,
        selection: snapshot.selection,
        frameOwners: snapshot.frameOwners
      };
      const serialized = serializeJsonBytesBounded(observation, options.maximumPayloadBytes);
      if (serialized.kind === "limit") {
        return payloadLimitError(options.captureId, options.maximumPayloadBytes, serialized.attempted);
      }
      const bytes = serialized.value;
      const calculatedChunks = mathCeil(typedArrayByteLength(bytes) / options.maximumChunkBytes);
      const chunkCount = calculatedChunks > 1 ? calculatedChunks : 1;
      const stored = {
        captureId: options.captureId,
        frameId: options.frameId,
        bytes,
        chunkCount,
        maximumChunkBytes: options.maximumChunkBytes,
        acknowledged: new SafeSet,
        sha256: await sha256(bytes)
      };
      mapSet(observations, key(options.captureId, options.frameId), stored);
      return collector.describe(options.captureId, options.frameId);
    },
    snapshotInline(request) {
      return snapshotInlineDocument(document, request);
    },
    positionVisualFallback: fallbackTargets.position,
    describe(captureId, frameId) {
      const stored = mapGet(observations, key(captureId, frameId));
      if (!stored) {
        throw new SafeTypeError("observation is not prepared");
      }
      return {
        captureId,
        frameId,
        chunks: stored.chunkCount,
        encodedBytes: typedArrayByteLength(stored.bytes),
        payloadSha256: stored.sha256
      };
    },
    read(captureId, frameId, sequence) {
      const stored = mapGet(observations, key(captureId, frameId));
      if (!stored || !numberIsSafeInteger(sequence) || sequence < 0 || sequence >= stored.chunkCount) {
        throw new SafeRangeError("observation chunk is unavailable");
      }
      const offset = sequence * stored.maximumChunkBytes;
      const payload = typedArraySubarray(stored.bytes, offset, offset + stored.maximumChunkBytes);
      const payloadValues = [];
      for (let index = 0;index < typedArrayByteLength(payload); index += 1) {
        arrayPush(payloadValues, payload[index]);
      }
      return {
        captureId,
        frameId,
        sequence,
        total: stored.chunkCount,
        payloadLength: typedArrayByteLength(payload),
        payloadCrc32: crc32(payload),
        payload: payloadValues
      };
    },
    acknowledge(captureId, frameId, sequence) {
      const stored = mapGet(observations, key(captureId, frameId));
      if (!stored || !numberIsSafeInteger(sequence) || sequence < 0 || sequence >= stored.chunkCount) {
        throw new SafeRangeError("observation chunk is unavailable");
      }
      setAdd(stored.acknowledged, sequence);
      return true;
    },
    release(captureId, frameId) {
      mapDelete(observations, key(captureId, frameId));
      return { captureId, frameId };
    }
  });
  defineProperty(globalThis, "__offprintCollector", {
    configurable: false,
    enumerable: false,
    writable: false,
    value: collector
  });
})();
