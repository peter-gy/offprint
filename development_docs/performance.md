# Performance evidence

The benchmark harness records machine details, browser identity, iteration
counts, minimums, medians, 95th percentiles, maximums, throughput, and
normalized medians in JSON.

```console
just benchmark offprint-rs/target/benchmark-evidence/performance.json
just benchmark-compare \
  offprint-rs/benches/baseline.json \
  offprint-rs/target/benchmark-evidence/performance.json \
  true
```

[`offprint-rs/benches/baseline.json`](../offprint-rs/benches/baseline.json) owns numerical baseline
claims. Documentation links to that record instead of copying values.

Each normalized median divides the case median by the SHA-256 calibration
median from the same process. The checked-in baseline records its original
environment, which can differ from a current checkout. The final `true` above
permits an exploratory cross-environment comparison and records the mismatch.
Release evidence needs a matching controlled environment or a reviewed
baseline regeneration.

Measure expansion of 2,000 CSS references into embedded resource URLs:

```console
cargo run --manifest-path offprint-rs/Cargo.toml --release --locked \
  -p offprint-bench --example css_expansion
```

This case checks exact rewritten text before measuring the rewrite alone. Its
roughly 8 MiB output exercises resource expansion beyond the small URLs in the
parse-and-rewrite microbenchmark.

Measure complete offline batches with 16 pages and 64 images per page at
concurrency 1, 4, and 8:

```console
cargo run --manifest-path offprint-rs/Cargo.toml --release --locked \
  -p offprint-bench --example batch_scaling
```

The example writes one JSON record per run, including stage timings and
per-job failures. It requires each rendered resource to be embedded and checks
artifact identity, result order, and zero-request offline verification.

Set `BATCH_IMAGES` to vary images per page from 1 through 4096 and `BATCH_PAGES`
to vary captures per run from 1 through 256. The defaults are 64 images and 16
pages. `BATCH_WIDTH` selects one concurrency value, and `BATCH_ITERATIONS`
sets the repeat count. The service retains its eight-context limit.

```console
BATCH_IMAGES=1024 BATCH_PAGES=8 BATCH_WIDTH=8 BATCH_ITERATIONS=3 \
  cargo run --manifest-path offprint-rs/Cargo.toml --release --locked \
  -p offprint-bench --example batch_scaling
```

## Repeated capture

The repeated-capture suite runs 32 captures through one service and managed
browser. It records artifact sizes and memory samples, verifies each capture's
zero-request offline reopen and expected rendered text, enforces Rust and
Chromium memory ceilings, and requires process and temporary-profile cleanup.

```console
cargo test --manifest-path offprint-rs/Cargo.toml --release --locked \
  -p offprint --test repeated_capture \
  -- --ignored --test-threads=1
```

Do not claim artifact-size or byte stability without a numerical criterion. The
suite currently records per-capture sizes and validates each artifact
independently.

Update the baseline after a reviewed change to navigation, collection,
resource acquisition, transformation, encoding, verification, scheduling, or
browser lifecycle.
