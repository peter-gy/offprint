# PageKnot contributor contract

PageKnot captures a rendered page through Chromium, transforms the observation
into safe-static HTML, verifies it with network access denied, and commits the
artifact through an atomic transaction.

## Architecture

| Area | Owner | Contract |
| --- | --- | --- |
| `crates/pageknot-model` | Canonical records | Public request, event, result, error, browser, resource, and artifact records |
| `crates/pageknot-browser` | Browser seam | Backend traits, sessions, network policy, and readiness observations |
| `crates/pageknot-chromium` | Chromium adapter | Discovery, managed installs, process ownership, CDP transport, targets, and collection |
| `crates/pageknot-protocol` | Collector protocol | Handshake, version negotiation, chunk envelopes, checksums, and message records |
| `crates/pageknot-capture` | Capture mechanics | Validation, state transitions, cancellation, budgets, resource graph, and content store |
| `crates/pageknot-document` | Document model | Arena DOM, discovery, CSS rewriting, state materialization, and sanitization |
| `crates/pageknot-html` | HTML artifact | Encoding, manifest embedding, structural repair, static verification, and fallbacks |
| `crates/pageknot-artifact` | Delivery | Bounded memory output and transactional file output |
| `crates/pageknot` | Service API | Runtime ownership, capture jobs, pipeline, diagnostics, browser service, inspect, and verify |
| `crates/pageknot-cli` | Command boundary | Commands, configuration precedence, output routing, diagnostics, and exit status |
| `collector` | Page observation | Document-start hooks and bounded pull-based collection |
| `bindings/node` | Node.js binding | ESM, CommonJS, async jobs, events, errors, and platform addon selection |
| `bindings/python` | Python binding | Async context manager, jobs, events, errors, and wheel module |
| `benches` | Performance evidence | Microbenchmarks, browser corpora, normalized comparison, and recorded baseline |
| `xtask` | Repository automation | Schema generation, CDP generation, fixture selection, and release checks |

The service crate owns orchestration. Parser, protocol, and CDP types stay
behind their owning crate boundaries.

## Domain vocabulary

- A **capture** is one request and one terminal result.
- A **job** is the cancellable handle for an active capture.
- An **observation** is browser state collected before transformation.
- A **resource reference** is one render-affecting URL at one document
  location.
- A **resource outcome** is `embedded`, `external`, `omitted`, or `failed`.
- An **artifact** is encoded output plus its embedded manifest.
- A **verification** is static validation or a network-denied browser reopen.
- A **managed browser** is a catalog entry whose revision and archive digest
  are pinned in `versions.toml`.

## Commands

```console
just fmt-check
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
- `crates/pageknot-chromium/src/cdp_generated.rs` comes from
  `cargo run -p xtask -- codegen-cdp`.
- `collector/dist/collector.js` and `collector.sha256` come from
  `bun run build` in `collector`.
- `crates/pageknot-chromium/generated/collector.js` and
  `collector.sha256` are the packaged copies from the same collector build.
- `benches/baseline.json` comes from `just benchmark
  benches/baseline.json`.

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

Every acquired browser lease, context, page, temporary store, event producer,
and staging artifact needs one explicit terminal owner. Cancellation, consumer
drop, encoding failure, verification failure, and shutdown must release those
owners.

Changes must not add stubs, weaken assertions, skip required fixtures, or alter
public contracts to hide a failure. Report the exact failing command and
evidence when a required environment is unavailable.

## Documentation

Write contract-shaped prose with project nouns. Put the working command or API
example near the behavior it demonstrates. Comments explain lifecycle order,
security boundaries, generated ownership, compatibility constraints, or the
reason for a bailout.
