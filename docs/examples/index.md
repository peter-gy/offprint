# Capture examples

Start with a file capture, then use the matching memory example when your
application needs bytes or progress events.

## Save a page

```console
offprint capture https://example.com --output example.html
```

The [quickstart](../start/quickstart.md) covers installation, opening the result,
and inspecting its evidence.

Complete programs in each language create a service, capture the page, and
close the service:

| Language | Write a file                                                                                                  | Return bytes and observe events                                                                                   | Setup                                           |
| -------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| Rust     | [`capture_file.rs`](https://github.com/peter-gy/offprint/blob/main/offprint-rs/core/examples/capture_file.rs) | [`capture_memory.rs`](https://github.com/peter-gy/offprint/blob/main/offprint-rs/core/examples/capture_memory.rs) | [Rust integration](../integrations/rust.md)     |
| Node.js  | [`capture.ts`](https://github.com/peter-gy/offprint/blob/main/sdk/node/examples/capture.ts)                   | [`capture-memory.ts`](https://github.com/peter-gy/offprint/blob/main/sdk/node/examples/capture-memory.ts)         | [Node.js integration](../integrations/node.md)  |
| Python   | [`capture.py`](https://github.com/peter-gy/offprint/blob/main/sdk/python/examples/capture.py)                 | [`capture_memory.py`](https://github.com/peter-gy/offprint/blob/main/sdk/python/examples/capture_memory.py)       | [Python integration](../integrations/python.md) |

## Adapt the capture

| Task                                           | Example                                                                                       |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Capture the first matching article             | [Selector capture](../guides/control-capture.md#choose-document-scope)                        |
| Wait for data and rendered content             | [Readiness](../guides/control-capture.md#choose-when-collection-begins)                       |
| Pass an authorization header or session cookie | [Authenticated pages](../guides/authenticated-pages.md)                                       |
| Fail when a rendering resource is missing      | [Resource policy](../guides/control-capture.md#choose-missing-resource-behavior)              |
| Capture a local HTML file                      | [Allowed file roots](../guides/control-capture.md#capture-local-files)                        |
| Permit a controlled private network            | [Custom network rules](../operations/security.md#network-policies)                            |
| Export PDF, Markdown, or an archive            | [Export representations](../guides/inspect-verify-export.md#export-alternate-representations) |
| Capture a list of URLs or follow links         | [Batch and crawl](../guides/batch-and-crawl.md)                                               |

## Inspect preservation evidence

The repository also contains browser fixtures for script-rendered content,
frames, shadow roots, stylesheets, form state, canvas pixels, resource failures,
and offline verification. Each fixture has one executable test owner.

After [contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md), run a named fixture
from the repository root:

```console
just test-fixture script-rendered-document
```

Browse the [fixture catalog](https://github.com/peter-gy/offprint/blob/main/fixtures/manifest/fixtures.json) and
[testing guide](https://github.com/peter-gy/offprint/blob/main/development_docs/testing.md) to check a specific
preservation contract or add a regression case.
