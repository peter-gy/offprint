# Limits and performance

Capture limits stop unbounded browser and document work. Choose values from the
page workload and host budget, then measure the resulting receipt timings and
resource counts.

## Default capture limits

| Limit | Default | What it counts |
| --- | ---: | --- |
| `duration` | 120 seconds | Complete capture deadline, including readiness delay and verification |
| `redirects` | 20 | Accepted navigation redirects |
| `frames` | 256 | Captured frames including the top-level document |
| `nodes` | 1,000,000 | Captured document nodes, also the compiled maximum |
| `resources` | 10,000 | Resource references, not unique URLs |
| `resource_bytes` | 64 MiB | One decoded resource body |
| `total_resource_bytes` | 512 MiB | Bytes admitted before content-digest deduplication |
| `collector_chunk_bytes` | 1 MiB | One collector protocol chunk |
| `concurrent_resources` | 8 | Simultaneous resource streams |
| `artifact_bytes` | 64 MiB | Final HTML or memory delivery and aggregate frame-observation payload |
| `resource_recursion_depth` | 64 | Recursive CSS and SVG resource traversal |
| `frame_depth` | 64 | Frame and recursively embedded HTML depth |

Every limit except `redirects` must be greater than zero. Zero redirects rejects
the first redirect. `total_resource_bytes` must be at least `resource_bytes`.

The receipt's `embeddedBytes` counts unique content digests, so it can be lower
than total received bytes. A declared HTTP content length is an early rejection
hint. Enforcement uses bytes actually received and decoded.

## Concurrency

One service defaults to four browser contexts and eight resource streams per
capture. Batch and crawl default to four concurrent captures.

Lower these values when file descriptors, memory, CPU, or remote-browser
capacity are constrained. Browser process memory and Rust process memory should
be measured separately.

## Measure a service workload

`CaptureReceipt.timings` reports validation, browser acquisition, navigation,
readiness, collection, resource, transform, encoding, verification, commit, and
total durations. Compare those values beside the receipt's resource and byte
counts and the artifact manifest's frame count.

Use one shared `Offprint` service when measuring repeated captures. Record the
browser source and version, capture profile, host, concurrency, and input set so
another run can reproduce the conditions.

Maintainers can use the repository benchmark and repeated-capture harnesses in
[Performance evidence](../../development_docs/performance.md).
