# Generated files

Edit each generated artifact through its owning source and generator.

| Output                                        | Source and command                                                                                                   |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `schemas/**`                                  | Rust records through `cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen`                 |
| `fixtures/manifest/**`                        | Fixture catalog through the same schema command                                                                      |
| `sdk/node/contracts.generated.d.ts`           | Rust schemas through the same schema command                                                                         |
| `sdk/python/src/offprint/contracts.py`        | Rust schemas through the same schema command                                                                         |
| `offprint-rs/chromium/src/cdp/generated/*.rs` | Pinned official CDP JSON through `cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen-cdp` |
| `collector/dist/collector.js` and digest      | Collector TypeScript through `pnpm --filter @offprint/collector build`                                               |
| Packaged collector copy and digest            | Same collector build copied into `offprint-chromium`                                                                 |
| `offprint-rs/benches/baseline.json`           | `just benchmark offprint-rs/benches/baseline.json`                                                                   |

Run:

```console
just codegen
just codegen-check
```

`codegen-check` compares generated output byte for byte. A generated diff needs
the owning source change in the same review.

## CDP generation

The CDP generator downloads pinned `browser_protocol.json` and
`js_protocol.json`, verifies their SHA-256 digests, selects the required types,
and records upstream provenance in generated headers.

The current generated module contains a narrow typed subset. The Chromium
adapter still uses typed local request records and raw method names for several
domains. Architecture decision records must describe that current boundary
rather than claiming complete generated command coverage.

## Documentation ownership

Generated inventories can own names, signatures, fields, enum values, and
defaults. Authored docs own mental models, examples, tradeoffs, security
consequences, and recovery.

Documentation changes that repeat generated facts need either a parity check or
a clear canonical owner.
