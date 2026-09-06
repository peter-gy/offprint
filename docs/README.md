# Offprint documentation

Offprint captures rendered pages as verified, self-contained artifacts. Start
with the [project quickstart](../README.md#quickstart) to build the CLI and
produce a verified HTML file.

The guides below cover the current alpha source checkout. The installed
`offprint <command> --help` output is the exact reference for available flags
and accepted values.

## Capture and operate

| Goal | Guide |
| --- | --- |
| Capture and verify one page | [Capture and verify](./cli.md#capture-and-verify-a-page) |
| Derive another artifact format | [Export another format](./cli.md#export-another-format) |
| Wait for client-side rendering | [Capture readiness](./cli.md#select-capture-readiness) |
| Capture one matching element | [Selector capture](./cli.md#capture-one-element) |
| Consume a versioned JSON result | [Machine-readable output](./cli.md#write-machine-readable-output) |
| Capture a batch or bounded link graph | [Batch and crawl](./cli.md#run-bounded-capture-sets) |
| Reuse browser, readiness, and resource settings | [Configuration](./configuration.md) |
| Pass request headers or cookies | [Credential inputs](./configuration.md#credential-inputs) |
| Recover from setup or capture failure | [Troubleshooting](./troubleshooting.md) |

## Use an API

| Interface | Guide |
| --- | --- |
| Rust | [Rust public API](./public-api.md) |
| Node.js | [`@offprint/node`](../bindings/node/README.md) |
| Python | [`offprint`](../bindings/python/README.md) |
| All interfaces | [Feature and parity matrix](./feature-matrix.md) |

The versioned request, result, error, and binding contracts live in
[`schemas/`](../schemas). Build the Rust reference from the current checkout
with `cargo doc --open -p offprint`.

## Understand the system

- [Concepts](./concepts.md) defines the product vocabulary and the path from a
  live page to a verified artifact.
- [Architecture](./architecture.md) maps the ports, adapters, composition root,
  capture flow, format flow, and dependency checks.
- [Security threat model](./threat-model.md) defines host, browser, network,
  credential, and artifact boundaries.
- [Performance baseline](./performance-baseline.md) defines the benchmark
  corpus and repeated-capture lifecycle reference.
- [Source provenance ledger](./provenance.md) records inspected sources,
  generated inputs, and managed browser archives.

## Maintain Offprint

- [Architecture decisions](./architecture-decisions.md) records accepted
  ownership and dependency choices.
- [Dependency evaluation](./dependencies.md) defines the dependency policy.
- [Release contract](./release.md) defines package contents and release gates.
- [Review guide](../REVIEW.md) routes contract, lifecycle, security, and
  generated-surface review.
- [Product and engineering specification](../SPEC.md) is the normative design
  and implementation record.

Run the documentation checks from the repository root:

```console
just repo-check
just docs-check
```
