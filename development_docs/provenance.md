# Acknowledgements and source inputs

- [SingleFile](https://github.com/gildas-lormeau/SingleFile) inspired Offprint's core
  idea of saving rendered web pages for offline use.
- [agent-browser](https://github.com/vercel-labs/agent-browser) informed practices
  for managing embedded browser engines.

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

Release assets include checksums, a software bill of materials, and managed
browser identity. The workflow submits hosted GitHub build provenance
attestations for the release archives.
