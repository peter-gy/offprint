# Run batches and crawls

Use a batch for a known list of independent capture requests. Use a crawl when
Offprint should discover links from one seed page.

Both schedulers preserve one terminal outcome per capture and can write a
resume manifest after each terminal result.

The batch example uses [jq](https://jqlang.org/), a command-line JSON query
tool, to construct a complete request from the generated fixture.

## Run a batch

A `BatchRequest` contains uniquely named batch job descriptors. Each descriptor
holds a complete `CaptureRequest`.

From a source checkout, use the canonical request example to create a complete
one-job batch:

```console
jq -n --slurpfile request schemas/examples/capture-request.json '{
  schemaVersion: 2,
  jobs: [{ id: "example", request: $request[0] }],
  concurrency: 1
}' > jobs.json

offprint batch jobs.json --json > batch-result.json
```

The batch request file is limited to 16 MiB. It rejects empty batches, duplicate
IDs, whitespace-only IDs or IDs longer than 256 characters, more than 100,000
jobs, and concurrency outside 1 through 256.

Capture profiles and most capture environment variables do not rewrite the
complete requests in `jobs.json`. CLI browser flags provide a default for
requests whose browser selection is `auto`. Explicit per-request selections
remain unchanged.

If some captures fail, Offprint writes `BatchResult` before returning exit
status `1`. Read each `outcomes` entry instead of treating process failure as a
missing result.

## Resume a batch

Add this `resume` field to the batch request:

```json
{
  "resume": {
    "manifest": "batch-resume.json",
    "retryFailed": false
  }
}
```

Resume requires file outputs and rejects requests containing credentials. A
successful prior outcome is reused only while its request digest and artifact
digest still match. Failed outcomes remain recorded unless `retryFailed` is
true.

## Crawl a site

```console
offprint crawl https://example.com \
  --output captures \
  --max-pages 100 \
  --max-depth 3 \
  --concurrency 4 \
  --resume crawl-resume.json \
  --json > crawl-result.json
```

The crawl uses deterministic breadth-first order. It follows HTTP and HTTPS
links from `a` and `area` elements, resolves relative links against the final
page URL, removes fragments, skips `rel=nofollow`, and sorts each discovered
set before scheduling it.

The default origin boundary keeps the crawl on the seed origin. Pass
`--allow-cross-origin` to schedule links on other origins. Page and depth limits
apply before new work is scheduled.

`maxPages` must be greater than zero, `maxDepth` cannot exceed 10,000, and
concurrency must be between 1 and 256.

Crawl output names combine breadth-first ordinal, a portable URL hint, and a
short URL digest. The CLI replaces an existing generated page path after the
new capture verifies successfully. Use a dedicated output directory when prior
captures must be retained. CLI crawl forces offline verification and requires a
local Offprint-owned browser. Service API crawls retain the seed request's
verification mode. A remote CDP endpoint is not accepted.

As with batch, partial failure writes `CrawlResult` before returning status
`1`. Use `--retry-failed` with `--resume` to schedule prior failures again.

## Distinguish the scheduler records

- `CaptureJob` is the cancellable handle for an active or completed capture.
- `BatchJob` is a serialized descriptor inside `BatchRequest`.
- `ResumeManifest` is the persisted scheduler checkpoint.
- `ScheduledCaptureOutcome` is one succeeded, failed, or resumed entry.

Result counters classify outcome and reuse separately. A resumed success is
counted in both `resumed` and `succeeded`. A resumed failure is counted in both
`resumed` and `failed`.

See the [record reference](../reference/records.md) for the stable camelCase
shapes.
