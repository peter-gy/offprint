# Offprint documentation

Offprint captures a rendered page as verified, self-contained HTML. Start with
the product model, complete one capture, then follow the path that matches your
next task.

## Start

1. [What is Offprint?](./start/what-is-offprint.md) defines the product, result,
   and preservation boundary.
2. [Why Offprint?](./start/why-offprint.md) explains why rendered pages need
   browser observation, resource resolution, verification, and transactional
   output.
3. [Install Offprint](./start/install.md) compares the CLI, Rust, Node.js, and
   Python distribution paths.
4. [Capture and verify one page](./start/quickstart.md) builds the CLI and
   produces an inspectable artifact.

## Concepts

- [The capture model](./concepts/capture-model.md) connects request, job,
  observation, artifact, verification, delivery, and receipt.
- [Artifacts and verification](./concepts/artifacts-and-verification.md) defines
  safe-static HTML, artifact manifests, and verification evidence.
- [Resources and fidelity](./concepts/resources-and-fidelity.md) explains
  references, records, outcomes, warnings, and retrieval provenance.
- [Browsers and ownership](./concepts/browsers.md) distinguishes browser
  selection, source, discovery, installation, process, and context ownership.

## Guides

| Task | Guide |
| --- | --- |
| Choose readiness, selection, resource handling, optimizations, or local files | [Control capture](./guides/control-capture.md) |
| Pass headers or cookies safely | [Authenticated pages](./guides/authenticated-pages.md) |
| Run known requests or discover a link graph | [Batch and crawl](./guides/batch-and-crawl.md) |
| Read manifests, repeat verification, or derive another representation | [Inspect, verify, and export](./guides/inspect-verify-export.md) |
| Install, select, list, or remove local browsers | [Manage browsers](./guides/manage-browsers.md) |
| Attach to caller-owned Chromium | [Remote browser](./guides/remote-browser.md) |
| Consume JSON, events, failures, and exit status | [Automation](./guides/automation.md) |

## Examples

[Executable capture scenarios](./examples/README.md) connect rendered inputs to
artifact state, receipt evidence, and the browser fixtures that enforce each
claim.

## Integrations

- [Rust](./integrations/rust.md)
- [Node.js](./integrations/node.md)
- [Python](./integrations/python.md)

The three language APIs call the same native service and share canonical
serialized records. Their construction, naming, type, and error ergonomics
differ by host language.

## Reference

- [CLI commands, output, and exit statuses](./reference/cli.md)
- [Configuration, profiles, environment variables, and defaults](./reference/configuration.md)
- [Requests, events, results, manifests, browser, batch, and crawl records](./reference/records.md)
- [Service construction, methods, lifecycle, and host-language differences](./reference/service-api.md)
- [Artifact format contracts and limits](./reference/formats.md)
- [Errors, stages, code families, and recovery](./reference/errors.md)
- [Platforms, versions, and interface parity](./reference/compatibility.md)

Generated JSON Schemas in [`schemas/`](../schemas) own exhaustive serialized
field shapes. Rustdoc, TypeScript declarations, and Python stubs own exact host
signatures.

## Operate

- [Security and trust boundaries](./operations/security.md)
- [Troubleshooting](./operations/troubleshooting.md)
- [Limits and performance](./operations/limits-and-performance.md)

## Develop Offprint

Contributor architecture, generated-file ownership, validation, dependency,
performance, provenance, decision, and release material lives in
[`development_docs/`](../development_docs/README.md).
