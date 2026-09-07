# Node.js and Python bindings

Both bindings map host objects to the same Rust `Offprint` service. They do not
own capture policy, scheduling, browser selection, artifact encoding, or
verification behavior.

```text
offprint-model -> canonical records and schemas
offprint       -> service behavior
      |-> napi-rs Node.js adapter
      `-> PyO3 Python adapter
```

## Source and package boundaries

Native adapters live in `offprint-rs/node` and `offprint-rs/python`, within the
same Cargo workspace as the service. `sdk/node` contains the npm host API and
package loader. `sdk/python/src/offprint` contains the Python host API and type
contracts. Maturin reads `sdk/python/pyproject.toml` and builds the separate
Rust manifest selected there.

`just node-package-check` installs a packed npm package and exercises Node.
`just python-wheel-check` builds a wheel from the extracted source distribution,
installs it in a fresh environment, and performs a browser capture. Cargo can
prune unrelated workspace packages from the source distribution's bundled
lockfile during its offline build.

## Shared surface

Each host exposes:

- Root `Offprint` runtime
- Capture, artifact, and browser services
- Shorthand file capture
- Defaulted request construction and complete `CaptureRequest` jobs
- Events, cancellation, batch, and crawl
- Inspect, HTML verify, export, and format verify
- Browser ensure, list, install, remove, doctor, and idle close
- Explicit shutdown and process-exit fallback

## Request construction

`captures.request(url, options)` builds a canonical `CaptureRequest` through
Rust's fluent `Capture` path. The request inherits the selected service profile.
The host receives an owned record it can edit, reuse, or submit to `start` and
`batch`. Constructing a request performs no browser work.

The file-capture shorthand and request factory share native option application.
Host code maps names and values. Rust owns defaults and validation.

## Node.js

napi-rs maps Rust futures into Promise-based tasks. ECMAScript module named
exports and the CommonJS entrypoint share `api.cjs`. The npm package contains
all four native addons, and `native.cjs` selects one by platform and
architecture.

Child services, jobs, and event iterators retain the root runtime. A
FinalizationRegistry requests cleanup for abandoned objects. Process exit uses a
blocking close fallback. `close()` and `Symbol.asyncDispose` are the orderly
contract.

`index.d.ts` owns host classes and option types. Generated
`contracts.generated.d.ts` owns canonical record aliases. ECMAScript modules
use named exports, while CommonJS returns the service object from
`require("offprint")`.

## Python

PyO3 and `pyo3-async-runtimes` release the Python runtime lock while awaiting
Rust work. The package exposes an async context manager, weak-reference
finalization, and an `atexit` blocking-close fallback.

Host method names and keyword arguments use snake_case. Canonical dictionaries
remain camelCase. Generated `contracts.py` defines importable `TypedDict` and
literal aliases, and the package root reexports the public contract names.

Python maps `ErrorStage` to exception subclasses while preserving canonical
error fields.

## Event delivery

Bindings create independent bounded asynchronous iterators. Rust event
journaling keeps capture execution independent of host callback speed. Each
iterator retains its native event stream and root runtime until iteration ends
or the iterator is released.

## Package evidence

Source tests verify API mapping, errors, events, cancellation, and lifecycle.
Package tests build a native artifact, install it into a clean environment, run
a real browser capture, inspect the manifest, and verify cleanup.

Current binding-boundary coverage is narrower than shared Rust coverage for
batch, crawl, browser administration, remote CDP, and several constructor
options. Keep the feature-evidence distinction explicit.
