# Offprint documentation

Offprint saves a rendered web page as self-contained HTML, verifies it with
network access denied, and returns evidence of what was captured.

[Install Offprint](./start/install.md), then
[capture your first page](./start/quickstart.md).

```console
offprint capture https://example.com --output example.html
```

## Choose your next task

| I want to… | Read |
| --- | --- |
| Decide whether Offprint preserves the content I need | [What is Offprint?](./start/what-is-offprint.md) |
| Understand the verification guarantees | [Why Offprint?](./start/why-offprint.md) |
| Wait for content, select an element, or capture local files | [Control capture](./guides/control-capture.md) |
| Capture a page that requires authentication | [Authenticated pages](./guides/authenticated-pages.md) |
| Capture a list of pages or follow a site's links | [Batch and crawl](./guides/batch-and-crawl.md) |
| Save documentation or articles as PDF | [Save a web page as PDF](./guides/export-pdf.md) |
| Read capture evidence or create PDF and other exports | [Inspect, verify, and export](./guides/inspect-verify-export.md) |
| Use JSON results, events, or cancellation | [Automation](./guides/automation.md) |
| Provision a browser | [Manage browsers](./guides/manage-browsers.md) |
| Connect to a browser running elsewhere | [Remote browser](./guides/remote-browser.md) |
| Start from a working program | [Examples](./examples/README.md) |

## Use Offprint in an application

Choose [Rust](./integrations/rust.md), [Node.js](./integrations/node.md), or
[Python](./integrations/python.md). Each interface calls the same native
service. Reuse one service for repeated captures and close it when your
application finishes.

## Understand the result

- [The capture model](./concepts/capture-model.md): requests, jobs, receipts,
  and service lifetime.
- [Artifacts and verification](./concepts/artifacts-and-verification.md): saved
  HTML, embedded manifests, and verification evidence.
- [Resources and fidelity](./concepts/resources-and-fidelity.md): embedded
  resources, missing content, and warnings.
- [Browsers and ownership](./concepts/browsers.md): browser selection, isolated
  contexts, and process lifetime.

## Look up a contract

- [CLI commands, output, and exit statuses](./reference/cli.md)
- [Configuration, profiles, environment variables, and defaults](./reference/configuration.md)
- [Request, event, result, and manifest records](./reference/records.md)
- [Service methods and lifecycle](./reference/service-api.md)
- [Artifact formats](./reference/formats.md)
- [Errors and recovery](./reference/errors.md)
- [Platforms, versions, and interface parity](./reference/compatibility.md)

## Run in production

- [Security and trust boundaries](./operations/security.md)
- [Troubleshooting](./operations/troubleshooting.md)
- [Limits and performance](./operations/limits-and-performance.md)

For build, test, architecture, and release work, use the
[contributor documentation](../development_docs/README.md).
