import {
  arrayPush,
  captureGetter,
  captureMethod,
  captureSetter,
  SafeTypeError,
} from "./primordials";

const nodePrototype = typeof Node === "undefined" ? undefined : Node.prototype;
const elementPrototype = typeof Element === "undefined" ? undefined : Element.prototype;
const documentPrototype = typeof Document === "undefined" ? undefined : Document.prototype;
const fragmentPrototype =
  typeof DocumentFragment === "undefined" ? undefined : DocumentFragment.prototype;
const templatePrototype =
  typeof HTMLTemplateElement === "undefined" ? undefined : HTMLTemplateElement.prototype;
const framePrototype =
  typeof HTMLIFrameElement === "undefined" ? undefined : HTMLIFrameElement.prototype;
const documentTypePrototype =
  typeof DocumentType === "undefined" ? undefined : DocumentType.prototype;
const nodeListPrototype = typeof NodeList === "undefined" ? undefined : NodeList.prototype;
const htmlCollectionPrototype =
  typeof HTMLCollection === "undefined" ? undefined : HTMLCollection.prototype;
const namedNodeMapPrototype =
  typeof NamedNodeMap === "undefined" ? undefined : NamedNodeMap.prototype;
const attrPrototype = typeof Attr === "undefined" ? undefined : Attr.prototype;
const shadowRootPrototype = typeof ShadowRoot === "undefined" ? undefined : ShadowRoot.prototype;
const parserPrototype = typeof DOMParser === "undefined" ? undefined : DOMParser.prototype;

export const SafeElement = typeof Element === "undefined" ? undefined : Element;
export const SafeHTMLElement = typeof HTMLElement === "undefined" ? undefined : HTMLElement;
export const SafeSVGElement = typeof SVGElement === "undefined" ? undefined : SVGElement;
export const SafeHTMLStyleElement =
  typeof HTMLStyleElement === "undefined" ? undefined : HTMLStyleElement;
export const SafeDOMParser = typeof DOMParser === "undefined" ? undefined : DOMParser;
export const capturedTreeWalkerNodes = 5;

export const nodeType = captureGetter<Node, number>(
  nodePrototype,
  "nodeType",
  (node) => node.nodeType,
);
export const firstChild = captureGetter<Node, ChildNode | null>(
  nodePrototype,
  "firstChild",
  (node) => node.firstChild,
);
export const nextSibling = captureGetter<Node, ChildNode | null>(
  nodePrototype,
  "nextSibling",
  (node) => node.nextSibling,
);
export const lastChild = captureGetter<Node, ChildNode | null>(
  nodePrototype,
  "lastChild",
  (node) => node.lastChild,
);
export const previousSibling = captureGetter<Node, ChildNode | null>(
  nodePrototype,
  "previousSibling",
  (node) => node.previousSibling,
);
export const parentNode = captureGetter<Node, ParentNode | null>(
  nodePrototype,
  "parentNode",
  (node) => node.parentNode,
);
export const ownerDocument = captureGetter<Node, Document | null>(
  nodePrototype,
  "ownerDocument",
  (node) => node.ownerDocument,
);
export const nodeValue = captureGetter<Node, string | null>(
  nodePrototype,
  "nodeValue",
  (node) => node.nodeValue,
);
export const nodeTextContent = captureGetter<Node, string | null>(
  nodePrototype,
  "textContent",
  (node) => node.textContent,
);
export const setNodeTextContent = captureSetter<Node, string | null>(
  nodePrototype,
  "textContent",
  (node, value) => {
    node.textContent = value;
  },
);
const getNodeChildNodes = captureGetter<Node, NodeListOf<ChildNode>>(
  nodePrototype,
  "childNodes",
  (node) => node.childNodes,
);
const getNodeBaseUri = captureGetter<Node, string>(
  nodePrototype,
  "baseURI",
  (node) => node.baseURI,
);
const getNodeNamespace = captureGetter<Node, string | null>(
  elementPrototype,
  "namespaceURI",
  (node) => (node as Node & { namespaceURI: string | null }).namespaceURI,
);
const getNodeLocalName = captureGetter<Node, string | null>(
  elementPrototype,
  "localName",
  (node) => (node as Node & { localName: string | null }).localName,
);

export const cloneNode = captureMethod<Node, [boolean], Node>(
  nodePrototype,
  "cloneNode",
  (node, deep) => node.cloneNode(deep),
);
export const appendChild = captureMethod<Node, [Node], Node>(
  nodePrototype,
  "appendChild",
  (node, child) => node.appendChild(child),
);
export const removeChild = captureMethod<Node, [Node], Node>(
  nodePrototype,
  "removeChild",
  (node, child) => node.removeChild(child),
);
export const replaceChild = captureMethod<Node, [Node, Node], Node>(
  nodePrototype,
  "replaceChild",
  (node, child, replaced) => node.replaceChild(child, replaced),
);

const elementQuerySelector = captureMethod<Element, [string], Element | null>(
  elementPrototype,
  "querySelector",
  (element, selector) => element.querySelector(selector),
);
const documentQuerySelector = captureMethod<Document, [string], Element | null>(
  documentPrototype,
  "querySelector",
  (document, selector) => document.querySelector(selector),
);
const fragmentQuerySelector = captureMethod<DocumentFragment, [string], Element | null>(
  fragmentPrototype,
  "querySelector",
  (fragment, selector) => fragment.querySelector(selector),
);
const elementQuerySelectorAll = captureMethod<Element, [string], NodeListOf<Element>>(
  elementPrototype,
  "querySelectorAll",
  (element, selector) => element.querySelectorAll(selector),
);
const documentQuerySelectorAll = captureMethod<Document, [string], NodeListOf<Element>>(
  documentPrototype,
  "querySelectorAll",
  (document, selector) => document.querySelectorAll(selector),
);
const fragmentQuerySelectorAll = captureMethod<DocumentFragment, [string], NodeListOf<Element>>(
  fragmentPrototype,
  "querySelectorAll",
  (fragment, selector) => fragment.querySelectorAll(selector),
);

export function querySelector(
  root: Document | DocumentFragment | Element,
  selector: string,
): Element | null {
  const type = nodeType(root);
  if (type === 9) {
    return documentQuerySelector(root as Document, selector);
  }
  if (type === 11) {
    return fragmentQuerySelector(root as DocumentFragment, selector);
  }
  return elementQuerySelector(root as Element, selector);
}

export function querySelectorAll(
  root: Document | DocumentFragment | Element,
  selector: string,
): Element[] {
  const type = nodeType(root);
  const nodes =
    type === 9
      ? documentQuerySelectorAll(root as Document, selector)
      : type === 11
        ? fragmentQuerySelectorAll(root as DocumentFragment, selector)
        : elementQuerySelectorAll(root as Element, selector);
  const result: Element[] = [];
  const length = nodeListLength(nodes);
  for (let index = 0; index < length; index += 1) {
    const node = nodeListItem(nodes, index);
    if (node) {
      arrayPush(result, node);
    }
  }
  return result;
}

export const setAttribute = captureMethod<Element, [string, string], void>(
  elementPrototype,
  "setAttribute",
  (element, name, value) => {
    element.setAttribute(name, value);
  },
);
export const getAttribute = captureMethod<Element, [string], string | null>(
  elementPrototype,
  "getAttribute",
  (element, name) => element.getAttribute(name),
);
export const hasAttribute = captureMethod<Element, [string], boolean>(
  elementPrototype,
  "hasAttribute",
  (element, name) => element.hasAttribute(name),
);
export const removeAttribute = captureMethod<Element, [string], void>(
  elementPrototype,
  "removeAttribute",
  (element, name) => {
    element.removeAttribute(name);
  },
);
export const toggleAttribute = captureMethod<Element, [string, boolean], boolean>(
  elementPrototype,
  "toggleAttribute",
  (element, name, force) => element.toggleAttribute(name, force),
);
export const contains = captureMethod<Node, [Node | null], boolean>(
  nodePrototype,
  "contains",
  (node, child) => node.contains(child),
);
export const elementBounds = captureMethod<Element, [], DOMRect>(
  elementPrototype,
  "getBoundingClientRect",
  (element) => element.getBoundingClientRect(),
);

const getElementAttributes = captureGetter<Element, NamedNodeMap>(
  elementPrototype,
  "attributes",
  (element) => element.attributes,
);
const getElementChildren = captureGetter<Element, HTMLCollection>(
  elementPrototype,
  "children",
  (element) => element.children,
);
const getElementShadowRoot = captureGetter<Element, ShadowRoot | null>(
  elementPrototype,
  "shadowRoot",
  (element) => element.shadowRoot,
);

export function namespaceUri(node: Node): string | null {
  return nodeType(node) === 1 ? getNodeNamespace(node) : null;
}

export function localName(node: Node): string | null {
  return nodeType(node) === 1 ? getNodeLocalName(node) : null;
}

export function childNodes(node: Node): ChildNode[] {
  const nodes = getNodeChildNodes(node);
  const result: ChildNode[] = [];
  const length = nodeListLength(nodes);
  for (let index = 0; index < length; index += 1) {
    const child = nodeListItem(nodes, index);
    if (child) {
      arrayPush(result, child);
    }
  }
  return result;
}

export function elementChildren(element: Element): Element[] {
  const children = getElementChildren(element);
  const result: Element[] = [];
  const length = htmlCollectionLength(children);
  for (let index = 0; index < length; index += 1) {
    const child = htmlCollectionItem(children, index);
    if (child) {
      arrayPush(result, child);
    }
  }
  return result;
}

export function attributeCount(element: Element): number {
  return namedNodeMapLength(getElementAttributes(element));
}

export function attributeAt(element: Element, index: number): Attr | null {
  return namedNodeMapItem(getElementAttributes(element), index);
}

export function attributeName(attribute: Attr): string {
  return attrName(attribute);
}

export function attributeValue(attribute: Attr): string {
  return attrValue(attribute);
}

export function shadowRoot(element: Element): ShadowRoot | null {
  return getElementShadowRoot(element);
}

const getDocumentElement = captureGetter<Document, HTMLElement>(
  documentPrototype,
  "documentElement",
  (document) => document.documentElement,
);
const getDocumentDoctype = captureGetter<Document, DocumentType | null>(
  documentPrototype,
  "doctype",
  (document) => document.doctype,
);
const getDocumentTitle = captureGetter<Document, string>(
  documentPrototype,
  "title",
  (document) => document.title,
);
const getDocumentCharacterSet = captureGetter<Document, string>(
  documentPrototype,
  "characterSet",
  (document) => document.characterSet,
);
const getDocumentDefaultView = captureGetter<Document, Window | null>(
  documentPrototype,
  "defaultView",
  (document) => document.defaultView,
);
const getDocumentBody = captureGetter<Document, HTMLElement | null>(
  documentPrototype,
  "body",
  (document) => document.body,
);
const getDocumentScrollingElement = captureGetter<Document, Element | null>(
  documentPrototype,
  "scrollingElement",
  (document) => document.scrollingElement,
);
const getDocumentStyleSheets = captureGetter<Document, StyleSheetList>(
  documentPrototype,
  "styleSheets",
  (document) => document.styleSheets,
);
const getDocumentAdoptedStyleSheets = captureGetter<Document, CSSStyleSheet[]>(
  documentPrototype,
  "adoptedStyleSheets",
  (document) => document.adoptedStyleSheets,
);
const getFragmentAdoptedStyleSheets = captureGetter<DocumentFragment, CSSStyleSheet[]>(
  shadowRootPrototype,
  "adoptedStyleSheets",
  (fragment) => (fragment as ShadowRoot).adoptedStyleSheets,
);

export const createElement = captureMethod<Document, [string], HTMLElement>(
  documentPrototype,
  "createElement",
  (document, name) => document.createElement(name),
);
export const createDocumentFragment = captureMethod<Document, [], DocumentFragment>(
  documentPrototype,
  "createDocumentFragment",
  (document) => document.createDocumentFragment(),
);
export const getSelection = captureMethod<Document, [], Selection | null>(
  documentPrototype,
  "getSelection",
  (document) => document.getSelection(),
);
export const createTreeWalker = captureMethod<Document, [Node, number], TreeWalker>(
  documentPrototype,
  "createTreeWalker",
  (document, root, show) => document.createTreeWalker(root, show),
);

export function documentElement(document: Document): HTMLElement {
  return getDocumentElement(document);
}

export function documentDoctype(document: Document): DocumentType | null {
  return getDocumentDoctype(document);
}

export function documentBaseUri(document: Document): string {
  return getNodeBaseUri(document);
}

export function documentTitle(document: Document): string {
  return getDocumentTitle(document);
}

export function documentCharacterSet(document: Document): string {
  return getDocumentCharacterSet(document);
}

export function documentDefaultView(document: Document): Window | null {
  return getDocumentDefaultView(document);
}

export function documentBody(document: Document): HTMLElement | null {
  return getDocumentBody(document);
}

export function documentScrollingElement(document: Document): Element | null {
  return getDocumentScrollingElement(document);
}

export function styleSheets(document: Document): StyleSheetList {
  return getDocumentStyleSheets(document);
}

export function adoptedStyleSheets(root: Document | ShadowRoot): CSSStyleSheet[] {
  return nodeType(root) === 9
    ? getDocumentAdoptedStyleSheets(root as Document)
    : getFragmentAdoptedStyleSheets(root as ShadowRoot);
}

const getTemplateContent = captureGetter<HTMLTemplateElement, DocumentFragment>(
  templatePrototype,
  "content",
  (template) => template.content,
);
const getFrameContentDocument = captureGetter<HTMLIFrameElement, Document | null>(
  framePrototype,
  "contentDocument",
  (frame) => frame.contentDocument,
);
const getFrameContentWindow = captureGetter<HTMLIFrameElement, Window | null>(
  framePrototype,
  "contentWindow",
  (frame) => frame.contentWindow,
);
const getShadowMode = captureGetter<ShadowRoot, ShadowRootMode>(
  shadowRootPrototype,
  "mode",
  (root) => root.mode,
);

export function templateContent(template: HTMLTemplateElement): DocumentFragment {
  return getTemplateContent(template);
}

export function frameContentDocument(frame: HTMLIFrameElement): Document | null {
  return getFrameContentDocument(frame);
}

export function frameContentWindow(frame: HTMLIFrameElement): Window | null {
  return getFrameContentWindow(frame);
}

export function shadowMode(root: ShadowRoot): ShadowRootMode {
  return getShadowMode(root);
}

const nodeListLength = captureGetter<NodeList, number>(
  nodeListPrototype,
  "length",
  (nodes) => nodes.length,
);
const nodeListItem = captureMethod<NodeList, [number], Node | null>(
  nodeListPrototype,
  "item",
  (nodes, index) => nodes.item(index),
);
const htmlCollectionLength = captureGetter<HTMLCollection, number>(
  htmlCollectionPrototype,
  "length",
  (collection) => collection.length,
);
const htmlCollectionItem = captureMethod<HTMLCollection, [number], Element | null>(
  htmlCollectionPrototype,
  "item",
  (collection, index) => collection.item(index),
);
const namedNodeMapLength = captureGetter<NamedNodeMap, number>(
  namedNodeMapPrototype,
  "length",
  (attributes) => attributes.length,
);
const namedNodeMapItem = captureMethod<NamedNodeMap, [number], Attr | null>(
  namedNodeMapPrototype,
  "item",
  (attributes, index) => attributes.item(index),
);
const attrName = captureGetter<Attr, string>(attrPrototype, "name", (attribute) => attribute.name);
const attrValue = captureGetter<Attr, string>(
  attrPrototype,
  "value",
  (attribute) => attribute.value,
);
const getDoctypeName = captureGetter<DocumentType, string>(
  documentTypePrototype,
  "name",
  (doctype) => doctype.name,
);
const getDoctypePublicId = captureGetter<DocumentType, string>(
  documentTypePrototype,
  "publicId",
  (doctype) => doctype.publicId,
);
const getDoctypeSystemId = captureGetter<DocumentType, string>(
  documentTypePrototype,
  "systemId",
  (doctype) => doctype.systemId,
);

export function doctypeName(doctype: DocumentType): string {
  return getDoctypeName(doctype);
}

export function doctypePublicId(doctype: DocumentType): string {
  return getDoctypePublicId(doctype);
}

export function doctypeSystemId(doctype: DocumentType): string {
  return getDoctypeSystemId(doctype);
}

const parseFromString = captureMethod<DOMParser, [string, DOMParserSupportedType], Document>(
  parserPrototype,
  "parseFromString",
  (parser, source, type) => parser.parseFromString(source, type),
);

export function parseHtml(source: string): Document {
  if (!SafeDOMParser) {
    throw new SafeTypeError("DOMParser is unavailable");
  }
  return parseFromString(new SafeDOMParser(), source, "text/html");
}

export function removeNode(node: Node): void {
  const parent = parentNode(node);
  if (parent) {
    removeChild(parent, node);
  }
}

export function replaceNode(node: Node, replacement: Node): void {
  const parent = parentNode(node);
  if (parent) {
    replaceChild(parent, replacement, node);
  }
}

export function isElement(node: Node): node is Element {
  return nodeType(node) === 1;
}
