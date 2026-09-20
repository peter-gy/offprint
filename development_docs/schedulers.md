# Batch, crawl, and resume

Schedulers sit above one-page capture. They create complete independent capture
requests, bound concurrency, aggregate terminal outcomes, and persist compact
resume state.

## Batch

`BatchRequest` contains uniquely named `BatchJob` descriptors. Each descriptor
owns one complete `CaptureRequest`. Validation requires:

- Public schema version agreement
- One through 100,000 descriptors
- Unique nonempty IDs
- Concurrency from 1 through 256
- Valid capture requests
- File output and no credentials when resume is enabled

The CLI's browser override replaces `BrowserSpec::Auto` and preserves explicit
per-request selections. Capture profile patches do not rewrite batch requests.

All descriptors reach a `ScheduledCaptureOutcome`. Partial failure still
returns `BatchResult`, after which the CLI maps the failed count to status `1`.
Outcomes retain input order even when jobs finish out of order. Pending-job
checkpoint bookkeeping runs when resume is configured.

## Crawl

`CrawlRequest` combines a seed capture request, output directory, page and depth
bounds, concurrency, same-origin policy, and optional resume settings.

The scheduler:

1. Captures the next breadth-first frontier entry.
2. Parses the committed HTML artifact.
3. Reads `a` and `area` links.
4. Skips `rel=nofollow`.
5. Resolves links against the final source URL.
6. Removes URL fragments and rejects non-HTTP schemes.
7. Applies the origin boundary and deterministic sort.
8. Persists the next frontier after terminal work.

Output names contain the breadth-first ordinal, a portable path hint, and a
short URL digest. The CLI forces offline verification. Service API crawl keeps
the seed request's verification mode. Crawl rejects remote CDP and can use the
local default or an injected backend.

## Plan and request identity

Batch plan identity hashes the ordered batch jobs. Crawl plan identity hashes
the raw seed request, output directory, page and depth bounds, and origin rule.
It excludes concurrency and resume options. URL-fragment removal happens after
the crawl plan digest. Resume state belongs to one schedule kind and plan
digest. A changed hashed input rejects an incompatible resume manifest.
Plan hashing streams canonical JSON through a bounded buffer into SHA-256.

Successful resume entries bind capture ID, artifact path, artifact digest, URL,
depth, and ordinal where applicable. Reuse requires the current file to match
the recorded digest. Failed entries are reused unless `retry_failed` is true.

## Persistence

`ResumeManifest` is the persisted checkpoint. Batch stores pending descriptor
IDs. Crawl stores a typed frontier with URL, depth, and ordinal. State updates
use artifact transactions after each terminal outcome.

Each resumable schedule holds a runtime-owned lease for its resolved checkpoint
destination, from loading through its final commit. Schedules targeting that
destination serialize within one `Offprint` service. Separate services and
processes must use separate checkpoint files. Destination identity resolves the
parent directory and retains the exact file name. Use one spelling for a
checkpoint name on case-insensitive filesystems.

The scheduler transfers the checkpoint to a blocking worker for bounded JSON
serialization and durable file commit, then receives ownership back. Reads and
writes share a 16 MiB limit, including the final newline. An oversized write
leaves the previous checkpoint intact.

The worker retains the lease when its caller is dropped. Runtime shutdown seals
checkpoint admission, cancels schedules, and waits for checkpoint owners within
the shutdown deadline. It reports a shutdown error if they cannot drain. Each
schedule awaits its commit before advancing, while other captures continue
processing browser events.

Keep the qualified names distinct:

- `CaptureJob`: handle for an active or completed capture
- `BatchJob`: serialized descriptor
- `ResumeJobRecord`: persisted terminal state
- `ScheduledCaptureOutcome`: returned aggregate entry
