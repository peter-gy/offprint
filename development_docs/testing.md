# Testing and validation

Run the package that owns the behavior after each change:

```console
just quick offprint-model
```

This checks package formatting, lints every target and feature with warnings
denied, and runs the default-feature tests. Dependencies compile as needed.
Ignored browser fixtures run through their dedicated recipes.

JavaScript static checks run through `just js-check`. The collector and Node
SDK use Vitest on Node. Python static checks run through `just python-qa`,
which combines Ruff, ty, and Pyrefly. `just python-check` builds the native
adapter and runs pytest. `just site-check` builds the VitePress site.

## Focus a failing check

```console
just check offprint-model
just lint offprint-model
just test offprint-model
```

`check` typechecks library, binary, test, example, and benchmark targets with
every feature enabled. `lint` applies the same scope through
[Clippy](https://doc.rust-lang.org/clippy/), Rust's linter. `test` accepts a
test-name substring as its second argument:

```console
just test offprint-model serialization
```

Cargo reports the matching test count. Use `test-fixture` for browser fixture
selection, which requires exactly one owner.

## Validate the workspace

Omit the package to run `check`, `lint`, or `test` across the workspace:

```console
just fmt-check
just repo-check
just workflow-check
just lint
just test
just codegen-check
just collector-check
just node-check
just node-package-check
just python-check
just python-wheel-check
just docs-check
just release-check
```

`just repo-check` checks handwritten source size, Cargo dependency direction,
documentation routes and links, version alignment, and package metadata.
`just release-check` runs the release gates, including extracted package checks
and dependency audits. Select browser groups, fuzzing, ownership checks, and
benchmarks by the change boundary.

`just docs-check` runs repository checks, builds Rust documentation with warnings
denied, executes documentation tests, compiles Rust examples, and checks the VitePress configuration and production site. Runtime and packaged binding examples run through
`node-check`, `node-package-check`, `python-check`, and `python-wheel-check`.

## Browser fixtures

Linux CI uses the [Chromium runtime action](../.github/actions/setup-chromium/action.yml)
to install browser libraries and an AppArmor profile for the managed Chromium
cache path. The profile permits the user namespaces Chromium needs for its
sandbox. See [Linux sandbox setup](../docs/operations/troubleshooting.md#chromium-reports-no-usable-sandbox-on-linux)
when reproducing startup failures on Ubuntu.
The headed-browser smoke suite runs under [Xvfb](https://www.x.org/releases/current/doc/man/man1/Xvfb.1.xhtml),
a virtual X display whose lifetime is bounded by the smoke command.

Use one owned fixture during development:

```console
just test-fixture FIXTURE_ID
```

Run one serialized group before handoff:

```console
just e2e GROUP
```

Fixture selection fails when an ID resolves to zero or multiple test owners.
Browser fixtures run with one test thread. Every external wait needs a deadline.

The fixture server provides controlled HTTP and HTTPS origins, redirects,
authentication, frames, service workers, compression, partial streams,
WebSocket and EventSource behavior, and oversized inputs.

## Choose the checks for your change

| Change                                             | Required evidence                                                    |
| -------------------------------------------------- | -------------------------------------------------------------------- |
| Public record or schema                            | Unit tests, codegen freshness, binding contracts                     |
| Browser adapter                                    | Focused test plus browser fixture group and process cleanup          |
| Parser, resource graph, serializer, or protocol    | Focused tests and `just fuzz-smoke`                                  |
| Foreign-function-interface-free ownership or state | Focused tests and `just miri`                                        |
| Node.js or Python                                  | Source tests and extracted package install capture                   |
| Export format                                      | Encoder verifier tests, artifact export integration, package mapping |
| Output transaction                                 | Failure injection, conflict modes, recovery, package path            |
| Measured capture path                              | Benchmark comparison and repeated-capture evidence                   |
| Workflow or release                                | Workflow lint, repository checks, package dry runs                   |

`just miri` uses [Miri](https://github.com/rust-lang/miri), a Rust interpreter
that checks memory behavior, to run library tests for x86-64 Linux. This target
matches CI across development hosts. Process-level contracts run as native
integration tests in the platform lanes.

Run `just repo-check` before every handoff. A focused package check compiles its
dependencies and tests the selected owner. Changes to a shared contract also
require the consumer suites listed in the table.

Package checks build and exercise extracted package graphs. Source-tree tests
do not establish package contents.

## Lifecycle review

Trace success, failure, cancellation, consumer drop, encoding failure,
verification failure, shutdown, and host exit. Confirm one terminal owner for:

- Browser process and remote connection
- Managed revision lease
- Context and page
- Target and transport task
- Event producer and subscriber
- Resource stream and content-store entry
- Temporary artifact and staging output
- Multi-output journal and backup

## Current conformance gap

The repository exercises Chromium, custom backends, and every representation
through focused tests. It does not yet expose the shared reusable backend and
representation conformance suites named by `AGENTS.md`. Keep that gap visible
until the common harnesses exist.
