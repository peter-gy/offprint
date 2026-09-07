# Architecture

The `offprint` crate is the production composition root. Dependencies point
toward canonical records and interface ports.

```text
CLI, Node.js, Python, Rust caller
                 |
             offprint
       /          |          \
browser ports  document    artifact delivery
     |          transform         |
Chromium       HTML and export   file or memory
adapter        formats           transaction
```

## Workspace ownership

`offprint-rs/Cargo.toml` owns the native product graph. Its `core` crate exposes
`offprint`, and `cli`, `node`, and `python` adapt that service to their hosts.
The public language packages live in `sdk/node` and `sdk/python`. They build
native adapters from the Rust workspace and keep host behavior in SDK source.

The root pnpm workspace groups `collector`, `sdk/node`, and `docs`. Shared
formatting and linting belong at that root. Build and runtime dependencies
belong to the package that consumes them. Python keeps its own uv lockfile and
QA configuration in `sdk/python/pyproject.toml`.

## Ownership

| Area                      | Owner                    | Contract                                                                                                                 |
| ------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Canonical records         | `offprint-model`         | Requests, policies, events, results, errors, browser, resources, artifacts, schemas                                      |
| Browser ports             | `offprint-browser`       | Backends, leases, contexts, pages, observations, network guard                                                           |
| Chromium adapter          | `offprint-chromium`      | Discovery, managed browser, process, CDP, targets, collector, resources, offline reopen                                  |
| Collector protocol        | `offprint-protocol`      | Handshake, capabilities, messages, chunks, checksums                                                                     |
| Capture primitives        | `offprint-capture`       | Validated values, reusable state and cancellation types, budgets, content store                                          |
| Document model            | `offprint-document`      | Arena DOM, reference discovery, CSS rewriting, state, sanitization                                                       |
| HTML format               | `offprint-html`          | Encoding, manifest, content policy, restoration programs, inspection, static verification                                |
| Safe-static orchestration | `offprint-transform`     | Document freeze, sanitization, repair, HTML encoding boundary                                                            |
| Export formats            | `offprint-export`        | Representation encoders and format-specific verifiers                                                                    |
| Delivery                  | `offprint-artifact`      | Memory writer, single-file transaction, multi-output journal and recovery                                                |
| Service runtime           | `offprint`               | Composition, production job control and resource state, pipeline, schedulers, browser and artifact services, diagnostics |
| Command boundary          | `offprint-cli`           | Parser, configuration, credentials, streams, rendering, signals, exit status                                             |
| Page observation          | `collector`              | Document-start hooks and bounded browser-state serialization                                                             |
| Host bindings             | `sdk/node`, `sdk/python` | Host objects, async tasks, errors, finalizers, native package entrypoints                                                |
| Automation                | `xtask`                  | Code generation, repository checks, fixture selection, packaging                                                         |

`offprint-document` exposes reusable reference and graph types. Production
capture materializes references through private service `ResourceState`.
`offprint-capture` exposes reusable cancellation and state primitives, while
production terminal arbitration belongs to service `JobControl`.

## Dependency direction

```text
offprint-model
  <- artifact, capture, document, protocol
  <- browser <- chromium
  <- document <- html <- transform and export
  <- offprint <- CLI and language bindings
```

`xtask check-repository` validates allowed Cargo edges. Model, port,
format-independent delivery, and frontend crates cannot depend on runtime
adapters.

## Composition root

`OffprintBuilder::build` creates `RuntimeState` with browser discovery or a
custom backend, context admission, process recycling, capture profiles, clock,
ID generation, artifact services, and schedulers.

`Offprint` and its three service views clone one `Arc<RuntimeState>`. Cloning a
service does not launch another browser. First browser use acquires the selected
local process or remote connection.

## Public and internal types

`offprint-model` contains canonical serialized records. The `offprint` facade
reexports an intentional service-facing set. Document-repair internals such as
`NodeId`, `RepairNode`, and `StructuralRepairTree` remain behind their owning
lower-crate boundary.

`offprint::ports` exposes the backend traits and their observation, resource,
and network types. An external adapter can implement the complete browser
contract through the service facade. The custom-backend integration suite
compiles and exercises that public path.

Several lower crates are published as dependency units and expose public Rust
types. Those types are not automatically part of the supported `offprint`
facade. CDP sessions and transaction journals remain adapter details even where
a dependency crate exposes construction primitives.

## Module boundaries

Handwritten production files stay below 990 lines. Split near 900 lines by
invariant, lifecycle owner, protocol phase, or representation boundary.
Generated files are exempt when their generator and freshness check are
recorded in [Generated files](./generated-files.md).
