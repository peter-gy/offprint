# Feature and parity matrix

This matrix maps the PageKnot contract to its implementation boundary and
release evidence.

| Capability | Rust API | CLI | Node.js | Python | Primary evidence |
| --- | --- | --- | --- | --- | --- |
| Safe-static HTML capture | `CaptureBuilder` | `capture` | `capture` | `capture` | `fixture_matrix` |
| File and bounded byte output | `save`, `to_bytes` | `--output` | output option | output option | artifact transaction tests |
| Static and offline verification | `verify` | `verify` | capture result | capture result | network-denied reopen fixtures |
| Artifact inspection | `inspect` | `inspect` | canonical result records | canonical result records | schema fixture suite |
| Artifact export | `export`, `verify_variant` | `export` | artifact service | artifact service | artifact variant integration suite |
| Capture jobs and events | `CaptureJob` | progress on stderr | async event iterator | async event iterator | binding contract suites |
| Cancellation | job and service ID | signal handling | job cancellation | job cancellation | stage cancellation matrix |
| Bounded batch scheduling | `CaptureService::batch` | `batch` | capture service | capture service | scheduler integration suite |
| Breadth-first crawling | `CaptureService::crawl` | `crawl` | capture service | capture service | scheduler resume suite |
| Local Chromium discovery | builder defaults | `doctor` | builder defaults | constructor defaults | Chromium discovery tests |
| Managed Chromium | builder default and `BrowserService` | first-use provisioning and `browser install/list/remove` | constructor default and browser service | constructor default and browser service | package capture, digest, and lease tests |
| Remote CDP static capture | explicit `CaptureRequest` | `capture`, `batch`, and `doctor` with `--cdp-url` | constructor plus explicit capture request | constructor plus explicit capture request | remote browser tests |
| Frames and OOPIFs | capture pipeline | capture | capture | capture | frame fixture group |
| Shadow DOM and CSSOM | collector protocol | capture | capture | capture | browser-state fixture group |
| Forms, canvas, and media | collector and fallbacks | capture | capture | capture | browser-state regressions |
| Resource graph and provenance | canonical result | JSON output | result DTO | result DTO | resource fixture group |
| Configuration precedence | builder and request | flags, environment, profile, config | explicit options | explicit options | CLI configuration tests |
| Sanitized diagnostics | builder | `--diagnostics` | structured error | structured exception | diagnostics tests |

SingleFile is the differential oracle for rendered state and offline behavior.
Artifact bytes are measured independently because the formats and
transformations differ.
