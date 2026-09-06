# Document model and collector

The browser collector observes state that does not survive in an HTML response.
Rust validates that observation, builds an arena-backed document and resource
graph, then produces the safe-static representation.

## Collector boundary

The TypeScript collector runs in the page at document start. Its hook records
closed shadow roots while preserving the page-visible `attachShadow` contract.
At capture time it creates a detached clone and reads:

- Rendered DOM and document metadata
- Open and observed closed shadow roots
- CSSOM and adopted stylesheets
- Form, disclosure, viewport, and scroll state
- Responsive image choices
- Canvas and media state
- Frame owner mappings and visual fallback rectangles
- Used-font observations and structured warnings

The collector does not choose capture policy, retrieve missing resources,
encode artifacts, write files, retry navigation, or verify output.

## Protocol

The handshake carries major and minor version, capture ID, build digests,
capabilities, and maximum chunk size. Negotiation currently checks protocol and
capabilities but does not validate the collector build digest. The host then
pulls data through:

```text
prepare -> describe -> read chunk -> acknowledge -> release
```

Each envelope binds capture and frame identity, sequence, total count, byte
length, and checksum. Payload and node budgets are reserved before the
collector grows serialized output.

## Arena document

`offprint-document` parses HTML with html5ever into an Offprint-owned arena.
Stable `NodeId` values support deterministic traversal, frame embedding,
selection pruning, state application, and serialization.

The document is an internal transformation model, not a general browser DOM.
Parsed, normalized, and verified values stay typed across internal boundaries.

## Resource graph

Discovery records every HTML, CSS, SVG, and embedded-document location that can
affect rendering. Each reference owns frame, node, location kind, original and
resolved URL, and rendering role.

CSS uses typed Lightning CSS discovery when possible and source-preserving
`cssparser` ranges for rewriting. CSS and SVG traversal share one resource
recursion limit. Frame depth, reference count, and byte limits apply
independently.

## State and visual fallbacks

Form values, selected options, open disclosure state, shadow content, adopted
stylesheets, canvas pixels, and media fallbacks are materialized into the
document. Canvas fallback tries collector `toDataURL`, then a clipped browser
screenshot, then returns a typed failure.

## Safe-static transform

Collection materializes captured form, shadow, frame, canvas, media, and
resource state into HTML. `offprint-transform` then proceeds leaf-first through
embedded frames:

1. Apply rendering-freeze styles.
2. Remove captured scripts, event handlers, JavaScript URLs, automatic refresh,
   and uncontrolled request triggers.
3. Detect browser-reparse instability and attach minimal structural repair.
4. Return a typed `SafeStaticDocument`.

`offprint-html` encodes the manifest, exact owned programs, and content security
policy, then reparses and statically verifies the produced bytes.

Changes to collector, document, resource, or transformation behavior require
the owning fixture, generated bundle freshness, recursive resource checks, and
fuzz smoke where a parser or serialized boundary changed.
