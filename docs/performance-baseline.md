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

## Recorded evidence

[`benches/baseline.json`](../benches/baseline.json) is the canonical recorded
benchmark. It contains the environment, browser identity, samples, medians, 95th
percentiles, throughput, and normalized measurements used by
`benchmark-compare`.

Keep numerical results in the JSON artifact so benchmark and documentation
values cannot drift apart.

## Repeated-capture lifecycle

The repeated-capture suite runs 32 captures through one `Offprint` service and
one managed browser. Each result must reopen with zero network requests, preserve
a stable artifact size, and release every owned Chromium process when the
service closes.

Run the lifecycle check with the pinned repository toolchain:

```console
cargo test --release --locked \
  -p offprint --test repeated_capture \
  -- --ignored --test-threads=1
```

The test writes its complete sample series and environment record to
`target/benchmark-evidence/repeated-capture.json`. The release guard permits at
most 256 MiB of Rust resident-memory growth after the first capture and at most
3 GiB of aggregate Chromium resident memory. It also requires zero owned
Chromium processes after service shutdown.

Record a new baseline through `just benchmark benches/baseline.json` after a
reviewed change to navigation, collection, resource acquisition,
transformation, encoding, verification, or browser lifecycle behavior.
