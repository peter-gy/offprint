# Feature and parity matrix

This matrix maps the current PageKnot alpha contract to each public interface
and to inspectable repository evidence.

| Capability | Rust API | CLI | Node.js | Python | Primary evidence |
| --- | --- | --- | --- | --- | --- |
| Safe-static HTML capture | `CaptureBuilder` | `capture` | `capture` | `capture` | [browser fixture matrix](../crates/pageknot/tests/fixture_matrix.rs) |
| File and bounded byte output | `save`, `to_bytes` | `--output` | `output` option | `output` option | [artifact transaction](../crates/pageknot-artifact/src/transaction.rs) |
| Static and offline verification | `verify` | `verify` | capture result | capture result | [browser fixture matrix](../crates/pageknot/tests/fixture_matrix.rs) |
| Artifact inspection | `inspect` | `inspect` | artifact service | artifact service | [artifact schemas](../schemas) |
| Artifact export | `export`, `verify_variant` | `export` | artifact service | artifact service | [artifact export integration](../crates/pageknot/tests/artifact_exports.rs) |
| Capture jobs and events | `CaptureJob` | progress on stderr | async event iterator | async event iterator | [Node.js contracts](../bindings/node/test/api.test.js), [Python contracts](../bindings/python/tests/test_api.py) |
| Cancellation | job and service ID | signal handling | job cancellation | job cancellation | [service backend contract](../crates/pageknot/tests/custom_backend.rs) |
| Bounded batch scheduling | `CaptureService::batch` | `batch` | capture service | capture service | [service backend contract](../crates/pageknot/tests/custom_backend.rs) |
| Breadth-first crawling | `CaptureService::crawl` | `crawl` | capture service | capture service | [service backend contract](../crates/pageknot/tests/custom_backend.rs) |
| Local Chromium discovery | builder defaults | `doctor` | builder defaults | constructor defaults | [browser runtime tests](../crates/pageknot/src/runtime/browser/tests.rs) |
| Managed Chromium | `BrowserService` | first-use provisioning and `browser` commands | browser service | browser service | [release archive smoke test](../crates/pageknot-cli/tests/release_archive_smoke.rs) |
| Remote CDP static capture | explicit `CaptureRequest` | `capture`, `batch`, `doctor` | constructor and request | constructor and request | [remote browser integration](../crates/pageknot/tests/remote_browser.rs) |
| Frames and out-of-process frames | capture pipeline | `capture` | `capture` | `capture` | [browser fixture matrix](../crates/pageknot/tests/fixture_matrix.rs) |
| Shadow DOM and CSS Object Model | collector protocol | `capture` | `capture` | `capture` | [capture regressions](../crates/pageknot/tests/capture_regressions.rs) |
| Forms, canvas, and media | collector and fallbacks | `capture` | `capture` | `capture` | [capture regressions](../crates/pageknot/tests/capture_regressions.rs) |
| Resource graph and provenance | canonical result | JSON output | result record | result dictionary | [browser fixture matrix](../crates/pageknot/tests/fixture_matrix.rs) |
| Configuration precedence | builder and request | flags, environment, profile, config | explicit options | explicit options | [CLI configuration tests](../crates/pageknot-cli/src/config/tests.rs) |
| Sanitized diagnostics | builder | `--diagnostics` | structured error | structured exception | [diagnostics integration](../crates/pageknot/tests/diagnostics.rs) |

SingleFile is the differential oracle for rendered state and offline behavior.
Artifact bytes are measured independently because the formats and
transformations differ. See the [provenance ledger](./provenance.md) for the
pinned revision and scope.
