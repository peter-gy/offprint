# Errors and recovery

`OffprintError` is the canonical failure record. Callers can branch on stable
codes and stages without matching message prose.

| Field             | Contract                                                                                          |
| ----------------- | ------------------------------------------------------------------------------------------------- |
| `code`            | Stable lowercase `offprint.*` identifier                                                          |
| `message`         | Redacted human description                                                                        |
| `stage`           | Owning failure stage                                                                              |
| `retryable`       | Whether retry can succeed without changing invalid input or policy                                |
| `details`         | Optional structured stage-specific values. Node.js and Python expose an empty mapping when absent |
| `diagnosticsPath` | Optional sanitized diagnostic bundle                                                              |
| `source`          | Optional nested `OffprintError`                                                                   |

## Error stages

- `validation`
- `browser`
- `navigation`
- `readiness`
- `collection`
- `resource`
- `transform`
- `encoding`
- `verification`
- `commit`
- `shutdown`
- `internal`

The stage classifies failure ownership. It does not correspond one-to-one with
`CaptureStatus`.

## Code families

| Family                    | Typical owner                                                      |
| ------------------------- | ------------------------------------------------------------------ |
| `offprint.input.*`        | Request, path, credentials, or schema input                        |
| `offprint.config.*`       | Configuration parsing and resolution                               |
| `offprint.browser.*`      | Discovery, launch, managed cache, CDP, or capability               |
| `offprint.navigation.*`   | Page navigation and redirect behavior                              |
| `offprint.readiness.*`    | Readiness condition or deadline                                    |
| `offprint.collector.*`    | Page-observation protocol                                          |
| `offprint.frame.*`        | Frame attachment or observation                                    |
| `offprint.resource.*`     | Resource retrieval, identity, stream, or limit                     |
| `offprint.transform.*`    | Document transformation                                            |
| `offprint.artifact.*`     | Artifact reading or model validation                               |
| `offprint.verification.*` | Static or offline rejection                                        |
| `offprint.output.*`       | Staging, conflict, commit, rollback, or recovery                   |
| `offprint.runtime.*`      | Cancellation, timeout, interruption, or closed service             |
| `offprint.internal.*`     | Contained internal defect                                          |
| `offprint.export.*`       | Export encoding, representation validation, or format verification |
| `offprint.scheduler.*`    | Batch, crawl, plan, resume, or partial failure                     |

Additional exact families cover bindings, selectors, active selection,
screenshots, and visual fallbacks. The registry is authoritative.

The exhaustive registry is [`schemas/error-codes.json`](https://github.com/peter-gy/offprint/blob/main/schemas/error-codes.json).

## Host-language mapping

Rust operations that can fail return `offprint::Result<T>`. Accessors and
non-failing report methods can return values directly.

Node.js rejects with one `OffprintError` class. Python raises stage-specific
subclasses of `OffprintError` while preserving the same record fields.

## Recovery order

1. Read `code`, `stage`, and `details`.
2. Apply the smallest input, browser, policy, or output correction.
3. Preserve artifacts and recovery paths named by output errors.
4. Retry only when `retryable` is true or the relevant input changed.
5. Run `offprint doctor --json` for browser or environment evidence.

Use [troubleshooting](../operations/troubleshooting.md) for symptom-led repair.
