# Performance evidence

The benchmark harness records machine details, browser identity, iteration
counts, minimums, medians, 95th percentiles, maximums, throughput, and
normalized medians in JSON.

```console
just benchmark target/benchmark-evidence/performance.json
just benchmark-compare \
  benches/baseline.json \
  target/benchmark-evidence/performance.json \
  true
```

[`benches/baseline.json`](../benches/baseline.json) owns numerical baseline
claims. Documentation links to that record instead of copying values.

Each normalized median divides the case median by the SHA-256 calibration
median from the same process. The checked-in baseline records its original
environment, which can differ from a current checkout. The final `true` above
permits an exploratory cross-environment comparison and records the mismatch.
Release evidence needs a matching controlled environment or a reviewed
baseline regeneration.

## Repeated capture

The repeated-capture suite runs 32 captures through one service and managed
browser. It records artifact sizes and memory samples, verifies each capture's
zero-request offline reopen and expected rendered text, enforces Rust and
Chromium memory ceilings, and requires process and temporary-profile cleanup.

```console
cargo test --release --locked \
  -p offprint --test repeated_capture \
  -- --ignored --test-threads=1
```

Do not claim artifact-size or byte stability without a numerical criterion. The
suite currently records per-capture sizes and validates each artifact
independently.

Update the baseline after a reviewed change to navigation, collection,
resource acquisition, transformation, encoding, verification, scheduling, or
browser lifecycle.
