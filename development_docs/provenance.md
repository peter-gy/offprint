# Source and license provenance

Offprint records inspected sources, adapted mechanisms, generated inputs, and
distributed binary inputs with immutable revisions or catalog versions.

| Source                                                                          | Revision                                   | License                   | Local use                                              |
| ------------------------------------------------------------------------------- | ------------------------------------------ | ------------------------- | ------------------------------------------------------ |
| [SingleFile](https://github.com/gildas-lormeau/SingleFile)                      | `7d556e71fa3a700d833116f77d31a8b9c3669643` | AGPL-3.0-or-later         | Behavior inventory and differential research           |
| [SingleFile CLI](https://github.com/gildas-lormeau/single-file-cli)             | `d1cdaa7b2006782415637ac79aeae136812826aa` | AGPL-3.0-or-later         | Scheduled differential executable                      |
| [agent-browser](https://github.com/vercel-labs/agent-browser)                   | `3cc7022271235694b5b5ce8aaea8bbfaa66e8cd5` | Apache-2.0                | Browser lifecycle and initial CDP mapping research     |
| [Bun](https://github.com/oven-sh/bun)                                           | `4eb6f99c1afeae07a7445218c25b3b742b643f11` | MIT and bundled licenses  | Workspace and native tooling research                  |
| [OpenAI Codex](https://github.com/openai/codex)                                 | `95637f7056835fea66bdd0044414af480fc0fd74` | Apache-2.0                | Workspace, ownership, and repository workflow research |
| [Chrome DevTools Protocol](https://github.com/ChromeDevTools/devtools-protocol) | `58bb3629dd105e4aae45f3e308cfd1564bdde91b` | BSD-3-Clause              | Selected generated protocol definitions                |
| [Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)    | Catalog `2026-07-27`, revision `1654411`   | Chrome distribution terms | Managed browser archives                               |

Offprint contains independently authored capture code. Direct inspection of
AGPL-licensed SingleFile informs the project's AGPL-3.0-or-later license
posture.

## Generated inputs

`versions.toml` records CDP input digests, managed browser revision, version,
catalog, and platform archive digests. Generated Rust headers record protocol
input provenance. Collector bundles record and compare their SHA-256 digests.

## Differential evidence

`just differential PATH` captures one hermetic fixture with Offprint and the
pinned SingleFile CLI under one managed Chromium build. It compares offline
request count, DOM and frame invariants, state, screenshot similarity, artifact
size, duration, and memory. The report also carries Offprint's resource summary,
but does not derive a SingleFile resource-outcome comparison. Artifact bytes are
measured, not expected to match.

`just exploratory-corpus` captures the versioned
[Datawrapper URL corpus](../fixtures/corpora/datawrapper.json) and writes
per-page outcomes. Live-site results are exploratory. Hermetic fixtures own
release gates.

## Release evidence

Release assets include dependency licenses, checksums, a software bill of
materials, and managed browser identity. The workflow submits hosted GitHub
artifact attestations for the release archives. Package metadata and the root
license must agree byte for byte where packaging requires it.
