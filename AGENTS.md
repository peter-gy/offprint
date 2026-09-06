# Offprint contributor contract

Offprint captures a rendered page through Chromium, transforms the observation
into safe-static HTML, applies static or offline verification, and delivers a
file or bounded memory value. Offline verification is the default.

## Architecture

| Area | Owner | Contract |
| --- | --- | --- |
| `crates/offprint-model` | Canonical records | Public request, event, result, error, browser, resource, and artifact records |
| `crates/offprint-browser` | Browser seam | Backend traits, sessions, network policy, and readiness observations |
| `crates/offprint-chromium` | Chromium adapter | Discovery, managed installs, process ownership, CDP transport, targets, and collection |
| `crates/offprint-protocol` | Collector protocol | Handshake, version negotiation, chunk envelopes, checksums, and message records |
| `crates/offprint-capture` | Capture primitives | Validation, reusable state and cancellation types, budgets, and content store |
| `crates/offprint-document` | Document model | Arena DOM, reference discovery, CSS rewriting, state materialization, and sanitization |
| `crates/offprint-html` | HTML artifact | Encoding, manifest embedding, owned restoration programs, content policy, structural repair, static verification, and fallbacks |
| `crates/offprint-transform` | Safe-static transform | Document freeze, sanitization, repair, and HTML format orchestration |
| `crates/offprint-export` | Alternate representations | Format encoders, source metadata preservation, and representation-specific verification |
| `crates/offprint-artifact` | Delivery | Bounded memory output and transactional file output |
| `crates/offprint` | Service API | Runtime ownership, production job control, resource materialization, pipeline, diagnostics, browser service, inspect, and verify |
| `crates/offprint-cli` | Command boundary | Commands, configuration precedence, output routing, diagnostics, and exit status |
| `collector` | Page observation | Document-start hooks and bounded pull-based collection |
| `bindings/node` | Node.js binding | ESM, CommonJS, async jobs, events, errors, and packaged native addon selection |
| `bindings/python` | Python binding | Async context manager, jobs, events, errors, and wheel module |
| `benches` | Performance evidence | Microbenchmarks, browser corpora, normalized comparison, and recorded baseline |
| `xtask` | Repository automation | Schema generation, CDP generation, fixture selection, and release checks |

The service crate is the production composition root. Parser, protocol, and
CDP types stay behind their owning crate boundaries.

## Dependency rule

Dependencies point toward canonical records and interface seams.

- `offprint-model` imports no Offprint crate.
- `offprint-browser` owns browser ports. `offprint-chromium` implements them
  and owns CDP, process, target, and managed-browser details.
- `offprint-document`, `offprint-html`, `offprint-transform`, and
  `offprint-export` depend on document and model contracts. They do not select
  runtime adapters.
- `offprint-artifact` stages, recovers, and commits generic payloads. Format
  encoders prepare payloads before delivery.
- `offprint` selects browser and format adapters and owns runtime
  orchestration.
- The CLI and language bindings call the `offprint` service API.

`xtask` validates the internal Cargo dependency graph. Reject changes that
introduce an adapter dependency into a port, model, frontend, or
format-independent delivery crate.

## Module boundaries

Keep each handwritten production source file below 990 lines. Review ownership
when a file approaches 900 lines, then split by invariant, lifecycle owner,
protocol phase, or representation boundary.

Generated files are exempt when their source generator and byte-for-byte
freshness check are recorded below. Test files may exceed the ceiling when one
contract suite remains easier to inspect as a unit. Do not satisfy the ceiling
with pass-through modules or arbitrary slices.

Keep parsed, normalized, and verified values typed across internal boundaries.
Parse and serialize at I/O boundaries. A representation encoder returns its
verified value after one verification pass. Standalone verification handles
artifacts loaded from storage.

Browser adapter changes run the Chromium and custom-backend contract suites.
Artifact representation changes run their encoder, verifier, service, and
browser integration tests. The reusable conformance harnesses are tracked as a
development gap in [`development_docs/testing.md`](./development_docs/testing.md).

## Domain vocabulary

- A **capture** is one request execution and one terminal result.
- A **capture job** is the cancellable handle for an active or completed
  capture.
- An **observation** is browser state collected before transformation.
- A **resource reference** is one render-affecting URL at one document
  location.
- A **resource outcome** is the terminal record for one reference. Production
  capture emits `embedded` or `failed`. The schema reserves `external` and
  `omitted` for future policies.
- An **Offprint HTML artifact** is the canonical encoded capture and its
  embedded artifact manifest.
- An **artifact delivery** is a committed file or bounded memory value.
- An **export format** is PDF, Markdown, ZIP, self-extracting HTML, or MHTML.
- A **verification mode** is static validation or a network-denied browser
  reopen.
- A **managed browser** is a catalog entry whose revision and archive digest
  are pinned in `versions.toml`.

## Commands

```console
just fmt-check
just clean
just lint
just test
just e2e GROUP
just exploratory-corpus
just benchmark
just codegen-check
just workflow-check
just docs-check
just release-check
```

Use `just test-fixture FIXTURE_ID` for one browser contract. Use
`just e2e GROUP` for one serialized fixture group. Browser fixtures consume a
Chromium process and must run with `--test-threads=1`.

Run `just exploratory-corpus` to capture the versioned Datawrapper URL corpus
through one browser service and write a machine-readable outcome report.
Run `just fuzz-smoke` after parser, serializer, resource graph, or protocol
changes. Run `just miri` after changes to FFI-free ownership or state code.
Run `just benchmark-compare benches/baseline.json` after changing a measured
capture path.

## Generated files

- `schemas/**` comes from `cargo run -p xtask -- codegen`.
- `fixtures/manifest/**` comes from the same schema command.
- `bindings/node/contracts.generated.d.ts` and
  `bindings/python/python/offprint/contracts.py` come from the same schema
  command.
- `crates/offprint-chromium/src/cdp/generated/*.rs` comes from
  `cargo run -p xtask -- codegen-cdp`.
- `collector/dist/collector.js` and `collector/dist/collector.sha256` come from
  `bun run build` in `collector`.
- `crates/offprint-chromium/generated/collector.js` and
  `collector.sha256` are the packaged copies from the same collector build.
- `benches/baseline.json` comes from the benchmark recipe with that file as its
  output.

Edit the source model, generator, collector source, or benchmark corpus.
`just codegen-check` must report byte-for-byte freshness.

## Security rules

- Treat page content, response metadata, URLs, headers, cookies, CDP endpoints,
  manifests, archives, and output paths as untrusted input.
- Apply byte and count limits before allocation or accumulation.
- Revalidate redirects and resolved addresses through `NetworkGuard`.
- Keep secrets out of command arguments, error messages, logs, JSON output,
  diagnostic bundles, and artifact provenance.
- Preserve browser sandboxing and process-tree ownership.
- Reject symlink ambiguity at managed-cache and artifact transaction
  boundaries.
- Commit a requested destination after verification succeeds.
- Keep captured scripts out of safe-static artifacts.
- Verify managed browser archives before extraction.

## Tests and review

Tests protect public records, lifecycle ownership, resource outcomes, output
transactions, and browser behavior. Browser appearance and offline behavior
require browser evidence.

Every required fixture has one executable owner. Fixture selection fails when
it resolves to zero tests or more than one test. Ignored browser tests need an
explicit fixture or workflow owner. Browser fixtures run serially and every
external wait has a deadline.

Package checks build and exercise the extracted package graph. Source-tree
tests do not establish package contents.

Every acquired browser lease, context, page, temporary store, event producer,
and staging artifact needs one explicit terminal owner. Cancellation, consumer
drop, encoding failure, verification failure, and shutdown must release those
owners.

Changes must not add stubs, weaken assertions, skip required fixtures, or alter
public contracts to hide a failure. Report the exact failing command and
evidence when a required environment is unavailable.

Before handoff, run the repository checks for handwritten file size and Cargo
dependency direction. Confirm that browser, server, page, process, stream, and
staging owners terminate on success, failure, cancellation, and consumer drop.

## Documentation

Write contract-shaped prose with project nouns. Put the working command or API
example near the behavior it demonstrates. Comments explain lifecycle order,
security boundaries, generated ownership, compatibility constraints, or the
reason for a bailout.

User documentation lives in [`docs/`](./docs/README.md). Current architecture,
pipeline, generation, validation, dependency, provenance, and release contracts
live in [`development_docs/`](./development_docs/README.md).
