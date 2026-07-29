# Source provenance ledger

PageKnot records inspected sources, adapted mechanisms, generated inputs, and
distributed binary inputs here. Revisions are immutable Git object IDs or
catalog versions.

| Source | Revision | License | PageKnot use |
| --- | --- | --- | --- |
| SingleFile | `7d556e71fa3a700d833116f77d31a8b9c3669643` | AGPL-3.0-or-later | Behavioral inventory, differential oracle, and safe page-capture research |
| SingleFile CLI | `d1cdaa7b2006782415637ac79aeae136812826aa` | AGPL-3.0-or-later | Pinned executable oracle for the scheduled differential fixture |
| agent-browser | `3cc7022271235694b5b5ce8aaea8bbfaa66e8cd5` | Apache-2.0 | Browser lifecycle research and the initial schema-mapping shape in `xtask/src/cdp.rs` |
| Bun | `4eb6f99c1afeae07a7445218c25b3b742b643f11` | MIT with separately licensed bundled components | Workspace and native-tooling research |
| OpenAI Codex | `95637f7056835fea66bdd0044414af480fc0fd74` | Apache-2.0 | Rust workspace, process ownership, and agent-facing repository research |
| Chrome DevTools Protocol | `58bb3629dd105e4aae45f3e308cfd1564bdde91b` | BSD-3-Clause | Generated definitions for thirteen selected domains |
| Chrome for Testing | catalog `2026-07-27`, revision `1654411`, version `151.0.7922.47` | Chrome distribution terms | Managed browser archives verified against `versions.toml` |

## Local adaptations

`xtask/src/cdp.rs` follows the data-shape mapping found in agent-browser
`cli/build.rs`. PageKnot adds exact upstream input hashes, selected domains,
command and event traits, stable formatting, a checked freshness mode, and
workspace-specific output.

PageKnot uses independently authored capture code. SingleFile remains a pinned
external differential oracle for rendered state and offline behavior. The
project license follows the AGPL-compatible posture required by the direct
source inspection recorded in `SPEC.md`.

## Generated definitions

The CDP generator downloads `browser_protocol.json` and
`js_protocol.json` from the pinned DevTools Protocol revision. It verifies:

- Browser protocol SHA-256
  `bb10379f95d76f9c423f68039df3bc996b8ae26563434a4d66e1bb0ae90300c8`
- JavaScript protocol SHA-256
  `5a54a335617a0ff088c22f8d7a39ee7616ebdba3eb982ebf4e6b1869239e60f5`

The generated Rust file records those values in its header. The collector
bundle records its own SHA-256 in `collector/dist/collector.sha256` and ships
from the identical generated copy in `crates/pageknot-chromium/generated`.

## Differential evidence

`just differential PATH` captures one hermetic fixture with PageKnot and the
pinned SingleFile CLI under the same managed Chromium build. It reopens both
artifacts with network access denied, compares browser-observed state and
viewport pixels, and records duration, artifact size, peak resident memory,
PageKnot resource outcomes, and browser identity under
`target/benchmark-evidence`.

`just exploratory-corpus` captures the 32 Datawrapper article URLs recorded in
`fixtures/corpora/datawrapper.json`. The report records the corpus digest,
browser identity, per-page outcome, artifact digest, resource summary, warning
codes, and timings. Live-site failures stay visible in the report while
synthetic fixtures own release gating.

## Managed browser archives

`versions.toml` stores one digest per catalog platform. Installation downloads
to bounded temporary storage, verifies the archive digest, validates extraction
paths, probes the browser version, and commits the revision under the managed
cache.

## Dependency licenses

`cargo deny check licenses advisories sources` evaluates Rust dependency
metadata. Bun lockfiles pin collector and binding development dependencies.
Release CI stores the dependency report with the release artifacts.
