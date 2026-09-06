# Release contract

Offprint publishes Rust crates, native CLI archives, a root npm package with
platform addon packages, and Python distributions from one signed version tag.
The tag version must match `versions.toml`, the Cargo workspace, every npm
package, and Python package metadata.

The version gate also compares the pinned Rust toolchain, the Rust and
TypeScript collector protocol declarations, and every managed-browser catalog
revision and archive digest with `versions.toml`.

Run the local release gate before creating the tag:

```console
just release-check
```

## Rust crates

The supported Rust API surface is `offprint`, `offprint-model`, and
`offprint-cli`. The `offprint` dependency graph also publishes its artifact,
browser, capture, Chromium, document, export, HTML, protocol, and transform
crates so Cargo can resolve the public packages from crates.io.

`cargo publish --workspace --locked` computes the dependency order and excludes
the workspace maintenance, binding, benchmark, and test-support packages.

## Native targets

| Target | Archive |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `x86_64-apple-darwin` | `.tar.gz` |
| `aarch64-apple-darwin` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `.zip` |

Each archive contains `offprint`, `README.md`, `LICENSE`,
`build-metadata.json`, and `SHA256SUMS`. CI publishes an outer SHA-256 digest,
an SBOM, `dependency-licenses.json`, and GitHub build provenance.

Each native matrix job builds from the tagged checkout, verifies that
`build-metadata.json` names the tag commit and target, provisions the pinned
browser in a fresh cache, then captures and offline-verifies a fixture through
the extracted executable.

## Release sequence

1. Run `just release-check`.
2. Run every required fixture group with the pinned managed browser.
3. Capture a hermetic page through each extracted CLI archive, packed npm
   package, and installed Python wheel.
4. Confirm `versions.toml`, package manifests, generated schemas, and binding
   versions agree.
5. Build the native matrix from the signed tag.
6. Generate the dependency license report, checksums, SBOM, and provenance.
7. Publish immutable artifacts.
8. Install each published shape and capture the static fixture.

The release gate requires all platform, browser, binding, dependency, Miri,
fuzz, sanitizer, repeated-capture, package-capture, and offline-verification
lanes. Scheduled assurance adds Chromium canary, sustained fuzz, differential,
and normalized performance comparison runs.
