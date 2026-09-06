# Architecture

Offprint runs one capture through provider-neutral browser ports, transforms the
observation into safe-static HTML, verifies the encoded artifact, and commits
the verified bytes. The `offprint` crate is the production composition root.

```text
CLI, Node.js, Python, Rust caller
               |
          offprint services
       /          |          \
browser ports  document    artifact delivery
     |          transform         |
Chromium       HTML and export   file or memory
adapter        formats   transaction
```

## Dependency direction

Dependencies point toward records and ports:

```text
offprint-model
  <- artifact, capture, document, protocol
  <- browser <- chromium
  <- document <- html <- transform and export
  <- offprint <- CLI and language bindings
```

`offprint-model` owns canonical public records. `offprint-browser` owns browser
ports and provider-neutral observation records. `offprint-protocol` owns the
collector handshake, commands, chunks, checksums, and negotiation. The browser
port has no protocol dependency. `offprint-chromium` translates Chrome DevTools
Protocol and collector messages into the browser port records.

`xtask check-repository` validates the exact allowed production edges. Adding a
workspace dependency requires an ownership decision in that matrix.

## Composition root

`Offprint::builder()` assembles the production runtime. The default assembly
creates the Chromium adapter, filesystem and memory artifact writers, the HTML
format, alternate format encoders, and the capture scheduler.
The runtime owns job admission, browser context limits, cancellation, recycling,
and shutdown.

Rust callers can replace capture browser behavior with
`OffprintBuilder::browser_backend`. A backend implements `BrowserBackend`,
`BrowserLease`, `BrowserContext`, and `PageSession` from `offprint-browser`.
Those ports exchange `FrameObservation`, bounded resource streams, readiness
evidence, and offline verification evidence. CDP sessions and collector wire
records stay inside `offprint-chromium`.

Managed browser installation, inventory, and removal use the production
Chromium adapter. A custom capture backend supplies its own `doctor` report for
the selected browser.

## Capture flow

1. `CaptureService` validates a `CaptureRequest` and registers one `CaptureJob`.
2. The runtime acquires a browser lease, context, and page through
   `BrowserBackend`.
3. `PageSession` navigates, settles, freezes rendering state, and returns frame
   observations.
4. The pipeline converts observations into `Document` values, resolves every
   render-affecting resource, and records one terminal outcome per reference.
5. Embedded CSS, SVG, and HTML resources are parsed and rewritten recursively.
   Active content is sanitized before bytes enter the artifact.
6. The transform freezes rendering, sanitizes the document, repairs unstable
   structure, and encodes the HTML format.
7. Static verification checks canonical metadata placement, active content,
   embedded resources, frame counts, digests, and the resource summary.
8. Offline verification reopens the staged artifact in a denied-network browser
   context when the request selects that policy.
9. The capture claims the terminal commit decision and atomically installs the
   verified file, or returns bounded bytes.

Cancellation, failure, and shutdown release page, context, lease, temporary
content, and staging owners in reverse acquisition order.

## Representation flow

HTML is the capture format and the verified source for export. The
artifact service revalidates source bytes, obtains offline evidence, then
dispatches each requested format to its encoder and verifier. A
multi-output artifact transaction validates names, entrypoints, digests, and
limits before installing the prepared set.

Adding a built-in format requires one model format, one encoder and
verifier, service dispatch, generated schemas and binding contracts, CLI
mapping, and format-level browser evidence.

## Validation boundaries

- `just repo-check` validates dependency direction, file size, package metadata,
  generated wiring, workflow pins, and local links.
- `just codegen-check` validates schemas, binding contracts, fixture manifests,
  selected CDP types, and collector bundles.
- `just test` exercises service, port, adapter, format, transaction,
  CLI, and binding behavior.
- `just e2e GROUP` runs one serialized browser fixture group.
- `just release-check` runs the source, package, binding, documentation,
  dependency, and generated-contract release gates. Required browser fixtures
  run through the serialized `e2e` groups in browser workflows.

Every required fixture names an explicit test owner. An unrecognized fixture ID
produces an invalid runner and fails the manifest contract test.
