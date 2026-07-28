# Performance baseline

The benchmark harness measures the parser, rewriting, resource, artifact, and
browser paths named in the product contract. It records machine details,
managed browser identity, sample counts, medians, 95th percentiles, throughput,
and normalized medians in one JSON report.

Run the complete corpus:

```console
just benchmark target/benchmark-evidence/performance.json
```

Compare the result with the recorded baseline:

```console
just benchmark-compare \
  benches/baseline.json \
  target/benchmark-evidence/performance.json
```

Normalized medians divide each sample by the SHA-256 calibration measured in
the same process. Scheduled CI permits cross-environment comparison with a
wider regression threshold and records whether the OS and architecture match.

## Benchmark corpus

The recorded macOS arm64 baseline uses Rust 1.97.0 and Chrome for Testing
151.0.7922.47 revision 1654411.

| Case | Median | 95th percentile |
| --- | ---: | ---: |
| HTML parse and serialize | 4.968 ms | 5.284 ms |
| CSS parse and URL rewrite | 3.167 ms | 3.259 ms |
| `srcset` parse | 0.481 ms | 0.499 ms |
| Data URL encode and decode | 1.797 ms | 1.826 ms |
| Resource graph creation and resolution | 1.246 ms | 1.276 ms |
| Content store hashing | 9.786 ms | 13.085 ms |
| Frame embedding | 0.119 ms | 0.129 ms |
| Manifest serialization | 0.001 ms | 0.001 ms |
| Static article capture | 3,288.584 ms | 3,321.236 ms |
| Frame-heavy capture | 3,084.543 ms | 3,148.295 ms |
| Image-heavy capture | 4,820.028 ms | 4,880.507 ms |
| Repeated capture through one service | 3,243.518 ms | 3,256.275 ms |

The complete samples and input sizes are stored in
[`benches/baseline.json`](../benches/baseline.json).

## Repeated-capture lifecycle

The repeated-capture suite runs 32 captures through one `PageKnot` service and
one managed browser. Each result must reopen with zero network requests, produce
the same artifact size, and release every owned Chromium process when the
service closes.

Run the baseline with the pinned toolchain:

```console
rustup run 1.97.0 cargo test --release --locked \
  -p pageknot --test repeated_capture \
  -- --ignored --test-threads=1
```

The test writes the complete sample series to
`target/benchmark-evidence/repeated-capture.json`.

### Reference environment

| Component | Value |
| --- | --- |
| Recorded | 2026-07-27 |
| Host | macOS 26.5.2, build 25F84 |
| Processor | Apple M3 Max |
| Physical memory | 38,654,705,664 bytes |
| Rust | 1.97.0 |
| Browser | Chrome for Testing 151.0.7922.47 |
| Browser revision | 1654411 |
| CDP protocol | 1.3 |
| Captures | 32 |

### Result

| Measure | Result |
| --- | ---: |
| Median capture | 2,424 ms |
| 95th percentile capture | 2,723 ms |
| Minimum capture | 2,393 ms |
| Maximum capture | 4,122 ms |
| Mean capture | 2,489.875 ms |
| Artifact size | 2,860 bytes for every capture |
| Warm Rust RSS | 12,107,776 bytes |
| Peak Rust RSS during capture | 13,795,328 bytes |
| Final Rust RSS | 13,926,400 bytes |
| Peak aggregate Chromium RSS | 1,689,468,928 bytes |
| Peak Chromium process count | 15 |
| Chromium processes after close | 0 |

The release guard allows at most 256 MiB of Rust RSS growth after the first
capture and at most 3 GiB of aggregate Chromium RSS. The recorded run remained
inside both bounds. The final process sample found no owned Chromium process.

Treat this file as the comparison point for changes to navigation, collection,
resource acquisition, transformation, encoding, verification, or browser
lifecycle behavior. Record the same environment fields and input fixture before
claiming a performance change.
